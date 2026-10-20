use crate::predict_edits::{
    PredictEditsRequestTrigger, RawCompletionChoice, RawCompletionRequest, RawCompletionResponse,
    RawCompletionUsage,
};
use clock::ReplicaId;
use db::AppDatabase;
use edit_prediction_types::EditPredictionRequestTrigger;
use futures::{
    AsyncReadExt, FutureExt, StreamExt,
    channel::{mpsc, oneshot},
};
use gpui::App;
use gpui::{
    Entity, TestAppContext, UpdateGlobal,
    http_client::{self, AsyncBody, FakeHttpClient, Method, Response},
};
use indoc::indoc;
use language::{
    Anchor, Buffer, Capability, Diagnostic, DiagnosticEntry, DiagnosticSet, DiagnosticSeverity,
    Point,
};
use lsp::LanguageServerId;
use parking_lot::Mutex;
use pretty_assertions::assert_eq;
use project::{FakeFs, Project};
use serde_json::json;
use settings::SettingsStore;
use std::{ops::Range, path::Path, sync::Arc, time::Duration};
use util::{
    path,
    test::{TextRangeMarker, marked_text_ranges_by},
};
use uuid::Uuid;
use workspace::{AppState, CollaboratorId, MultiWorkspace};
use zeta_prompt::{
    Zeta2PromptInput,
    zeta1::{CURSOR_MARKER, EDITABLE_REGION_END_MARKER, EDITABLE_REGION_START_MARKER},
};

use crate::prediction::EditPredictionInputs;
use crate::udiff::apply_diff_to_string;
use crate::{EditPredictionId, EditPredictionStore};

use super::*;

#[gpui::test]
async fn test_refresh_prediction_from_buffer_honors_debounce_duration(cx: &mut TestAppContext) {
    let (ep_store, mut requests) = init_test_with_fake_client(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        "/root",
        json!({
            "foo.md": "Hello!\n"
        }),
    )
    .await;

    let project = Project::test(fs, vec![path!("/root").as_ref()], cx).await;

    let buffer = project
        .update(cx, |project, cx| {
            let path = project.find_project_path(path!("root/foo.md"), cx).unwrap();
            project.open_buffer(path, cx)
        })
        .await
        .unwrap();
    let snapshot = buffer.read_with(cx, |buffer, _cx| buffer.snapshot());
    let position = snapshot.anchor_before(language::Point::new(0, 0));

    ep_store.update(cx, |ep_store, cx| {
        ep_store.register_project(&project, cx);
        ep_store.register_buffer(&buffer, &project, cx);
        ep_store.refresh_prediction_from_buffer(
            project.clone(),
            buffer.clone(),
            position,
            Duration::from_millis(100),
            EditPredictionRequestTrigger::Other,
            cx,
        );
    });
    cx.run_until_parked();

    // The configured debounce duration should prevent the request from being sent immediately.
    assert_no_predict_request_ready(&mut requests.predict);

    cx.background_executor
        .advance_clock(Duration::from_millis(150));
    cx.run_until_parked();

    let (request, respond_tx) = requests.predict.next().await.unwrap();
    respond_tx
        .send(model_response(
            &request,
            "--- a/root/foo.md\n+++ b/root/foo.md\n@@ ... @@\n Hello!\n+world\n",
        ))
        .unwrap();

    cx.run_until_parked();
}

#[gpui::test]
async fn test_refresh_prediction_from_buffer_suppressed_while_following(cx: &mut TestAppContext) {
    let (ep_store, mut requests) = init_test_with_fake_client(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        "/root",
        json!({
            "foo.md":  "Hello!\nHow\nBye\n"
        }),
    )
    .await;
    let project = Project::test(fs, vec![path!("/root").as_ref()], cx).await;

    let app_state = cx.update(|cx| {
        let app_state = AppState::test(cx);
        AppState::set_global(app_state.clone(), cx);
        app_state
    });
    let multi_workspace =
        cx.add_window(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
    let workspace = multi_workspace
        .read_with(cx, |multi_workspace, _| multi_workspace.workspace().clone())
        .unwrap();
    cx.update(|cx| {
        AppState::set_global(workspace.read(cx).app_state().clone(), cx);
    });
    drop(app_state);

    let buffer = project
        .update(cx, |project, cx| {
            let path = project.find_project_path(path!("root/foo.md"), cx).unwrap();
            project.open_buffer(path, cx)
        })
        .await
        .unwrap();
    let snapshot = buffer.read_with(cx, |buffer, _cx| buffer.snapshot());
    let position = snapshot.anchor_before(language::Point::new(1, 3));

    multi_workspace
        .update(cx, |multi_workspace, window, cx| {
            multi_workspace.workspace().update(cx, |workspace, cx| {
                workspace.start_following(CollaboratorId::Agent, window, cx);
            });
        })
        .unwrap();
    cx.run_until_parked();

    ep_store.update(cx, |ep_store, cx| {
        ep_store.register_project(&project, cx);
        ep_store.register_buffer(&buffer, &project, cx);
        ep_store.refresh_prediction_from_buffer(
            project.clone(),
            buffer.clone(),
            position,
            Duration::ZERO,
            EditPredictionRequestTrigger::Other,
            cx,
        );
    });
    cx.run_until_parked();

    assert_no_predict_request_ready(&mut requests.predict);
}

#[gpui::test]
async fn test_simple_request(cx: &mut TestAppContext) {
    let (ep_store, mut requests) = init_test_with_fake_client(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        "/root",
        json!({
            "foo.md":  "Hello!\nHow\nBye\n"
        }),
    )
    .await;
    let project = Project::test(fs, vec![path!("/root").as_ref()], cx).await;

    let buffer = project
        .update(cx, |project, cx| {
            let path = project.find_project_path(path!("root/foo.md"), cx).unwrap();
            project.open_buffer(path, cx)
        })
        .await
        .unwrap();
    let snapshot = buffer.read_with(cx, |buffer, _cx| buffer.snapshot());
    let position = snapshot.anchor_before(language::Point::new(1, 3));

    let prediction_task = ep_store.update(cx, |ep_store, cx| {
        ep_store.request_prediction(
            &project,
            &buffer,
            position,
            PredictEditsRequestTrigger::Other,
            cx,
        )
    });

    let (request, respond_tx) = requests.predict.next().await.unwrap();

    // TODO Put back when we have a structured request again
    // assert_eq!(
    //     request.excerpt_path.as_ref(),
    //     Path::new(path!("root/foo.md"))
    // );
    // assert_eq!(
    //     request.cursor_point,
    //     Point {
    //         line: Line(1),
    //         column: 3
    //     }
    // );

    respond_tx
        .send(model_response(
            &request,
            indoc! { r"
                --- a/root/foo.md
                +++ b/root/foo.md
                @@ ... @@
                 Hello!
                -How
                +How are you?
                 Bye
            "},
        ))
        .unwrap();

    let prediction = prediction_task.await.unwrap().unwrap().prediction;

    assert_eq!(prediction.edits.len(), 1);
    assert_eq!(
        prediction.edits[0].0.to_point(&snapshot).start,
        language::Point::new(1, 3)
    );
    assert_eq!(prediction.edits[0].1.as_ref(), " are you?");
}

#[gpui::test]
async fn test_request_events(cx: &mut TestAppContext) {
    let (ep_store, mut requests) = init_test_with_fake_client(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        "/root",
        json!({
            "foo.md": "Hello!\n\nBye\n"
        }),
    )
    .await;
    let project = Project::test(fs, vec![path!("/root").as_ref()], cx).await;

    let buffer = project
        .update(cx, |project, cx| {
            let path = project.find_project_path(path!("root/foo.md"), cx).unwrap();
            project.open_buffer(path, cx)
        })
        .await
        .unwrap();

    ep_store.update(cx, |ep_store, cx| {
        ep_store.register_buffer(&buffer, &project, cx);
    });

    buffer.update(cx, |buffer, cx| {
        buffer.edit(vec![(7..7, "How")], None, cx);
    });

    let snapshot = buffer.read_with(cx, |buffer, _cx| buffer.snapshot());
    let position = snapshot.anchor_before(language::Point::new(1, 3));

    let prediction_task = ep_store.update(cx, |ep_store, cx| {
        ep_store.request_prediction(
            &project,
            &buffer,
            position,
            PredictEditsRequestTrigger::Other,
            cx,
        )
    });

    let (request, respond_tx) = requests.predict.next().await.unwrap();

    let prompt = prompt_from_request(&request);
    assert!(
        prompt.contains(indoc! {"
        User edited root/foo.md:
        ```diff
        @@ -1,3 +1,3 @@
         Hello!
        -
        +How
         Bye
    "}),
        "{prompt}"
    );

    respond_tx
        .send(model_response(
            &request,
            indoc! {r#"
                --- a/root/foo.md
                +++ b/root/foo.md
                @@ ... @@
                 Hello!
                -How
                +How are you?
                 Bye
        "#},
        ))
        .unwrap();

    let prediction = prediction_task.await.unwrap().unwrap().prediction;

    assert_eq!(prediction.edits.len(), 1);
    assert_eq!(prediction.edits[0].1.as_ref(), " are you?");
}

#[gpui::test]
async fn test_edit_history_getter_pause_splits_last_event(cx: &mut TestAppContext) {
    let (ep_store, _requests) = init_test_with_fake_client(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        "/root",
        json!({
            "foo.md": "Hello!\n\nBye\n"
        }),
    )
    .await;
    let project = Project::test(fs, vec![path!("/root").as_ref()], cx).await;

    let buffer = project
        .update(cx, |project, cx| {
            let path = project.find_project_path(path!("root/foo.md"), cx).unwrap();
            project.open_buffer(path, cx)
        })
        .await
        .unwrap();

    ep_store.update(cx, |ep_store, cx| {
        ep_store.register_buffer(&buffer, &project, cx);
    });

    // First burst: insert "How"
    buffer.update(cx, |buffer, cx| {
        buffer.edit(vec![(7..7, "How")], None, cx);
    });

    // Simulate a pause longer than the grouping threshold (e.g. 500ms).
    cx.executor().advance_clock(LAST_CHANGE_GROUPING_TIME * 2);
    cx.run_until_parked();

    // Second burst: append " are you?" immediately after "How" on the same line.
    //
    // Keeping both bursts on the same line ensures the existing line-span coalescing logic
    // groups them into a single `LastEvent`, allowing the pause-split getter to return two diffs.
    buffer.update(cx, |buffer, cx| {
        buffer.edit(vec![(10..10, " are you?")], None, cx);
    });

    // A second edit shortly after the first post-pause edit ensures the last edit timestamp is
    // advanced after the pause boundary is recorded, making pause-splitting deterministic.
    buffer.update(cx, |buffer, cx| {
        buffer.edit(vec![(19..19, "!")], None, cx);
    });

    // With time-based splitting, there are two distinct events.
    let events = ep_store.update(cx, |ep_store, cx| {
        ep_store.edit_history_for_project(&project, cx)
    });
    assert_eq!(events.len(), 2);

    let first_total_edit_range = buffer.read_with(cx, |buffer, _| {
        events[0].total_edit_range.to_point(&buffer.snapshot())
    });
    assert_eq!(first_total_edit_range, Point::new(1, 0)..Point::new(1, 3));

    let zeta_prompt::Event::BufferChange { diff, .. } = events[0].event.as_ref();
    assert_eq!(
        diff.as_str(),
        indoc! {"
            @@ -1,3 +1,3 @@
             Hello!
            -
            +How
             Bye
        "}
    );

    let second_total_edit_range = buffer.read_with(cx, |buffer, _| {
        events[1].total_edit_range.to_point(&buffer.snapshot())
    });
    assert_eq!(second_total_edit_range, Point::new(1, 3)..Point::new(1, 13));

    let zeta_prompt::Event::BufferChange { diff, .. } = events[1].event.as_ref();
    assert_eq!(
        diff.as_str(),
        indoc! {"
            @@ -1,3 +1,3 @@
             Hello!
            -How
            +How are you?!
             Bye
        "}
    );
}

#[gpui::test]
async fn test_predicted_edits_are_separated_in_edit_history(cx: &mut TestAppContext) {
    let (ep_store, _requests) = init_test_with_fake_client(cx);
    let fs = FakeFs::new(cx.executor());

    // Create a file with 30 lines to test line-based coalescing
    let content = (1..=30)
        .map(|i| format!("Line {}\n", i))
        .collect::<String>();
    fs.insert_tree(
        "/root",
        json!({
            "foo.md": content
        }),
    )
    .await;
    let project = Project::test(fs, vec![path!("/root").as_ref()], cx).await;

    let buffer = project
        .update(cx, |project, cx| {
            let path = project.find_project_path(path!("root/foo.md"), cx).unwrap();
            project.open_buffer(path, cx)
        })
        .await
        .unwrap();

    ep_store.update(cx, |ep_store, cx| {
        ep_store.register_buffer(&buffer, &project, cx);
    });

    // First edit: multi-line edit spanning rows 10-12 (replacing lines 11-13)
    buffer.update(cx, |buffer, cx| {
        let start = Point::new(10, 0).to_offset(buffer);
        let end = Point::new(13, 0).to_offset(buffer);
        buffer.edit(vec![(start..end, "Middle A\nMiddle B\n")], None, cx);
    });

    let events = ep_store.update(cx, |ep_store, cx| {
        ep_store.edit_history_for_project(&project, cx)
    });
    assert_eq!(
        render_events(&events),
        indoc! {"
            @@ -8,9 +8,8 @@
             Line 8
             Line 9
             Line 10
            -Line 11
            -Line 12
            -Line 13
            +Middle A
            +Middle B
             Line 14
             Line 15
             Line 16
        "},
        "After first edit"
    );

    // Second edit: insert ABOVE the first edit's range (row 5, within 8 lines of row 10)
    // This tests that coalescing considers the START of the existing range
    buffer.update(cx, |buffer, cx| {
        let offset = Point::new(5, 0).to_offset(buffer);
        buffer.edit(vec![(offset..offset, "Above\n")], None, cx);
    });

    let events = ep_store.update(cx, |ep_store, cx| {
        ep_store.edit_history_for_project(&project, cx)
    });
    assert_eq!(
        render_events(&events),
        indoc! {"
            @@ -3,14 +3,14 @@
             Line 3
             Line 4
             Line 5
            +Above
             Line 6
             Line 7
             Line 8
             Line 9
             Line 10
            -Line 11
            -Line 12
            -Line 13
            +Middle A
            +Middle B
             Line 14
             Line 15
             Line 16
        "},
        "After inserting above (should coalesce)"
    );

    // Third edit: insert BELOW the first edit's range (row 14 in current buffer, within 8 lines of row 12)
    // This tests that coalescing considers the END of the existing range
    buffer.update(cx, |buffer, cx| {
        let offset = Point::new(14, 0).to_offset(buffer);
        buffer.edit(vec![(offset..offset, "Below\n")], None, cx);
    });

    let events = ep_store.update(cx, |ep_store, cx| {
        ep_store.edit_history_for_project(&project, cx)
    });
    assert_eq!(
        render_events(&events),
        indoc! {"
            @@ -3,15 +3,16 @@
             Line 3
             Line 4
             Line 5
            +Above
             Line 6
             Line 7
             Line 8
             Line 9
             Line 10
            -Line 11
            -Line 12
            -Line 13
            +Middle A
            +Middle B
             Line 14
            +Below
             Line 15
             Line 16
             Line 17
        "},
        "After inserting below (should coalesce)"
    );

    // Fourth edit: insert FAR BELOW (row 25, beyond 8 lines from the current range end ~row 15)
    // This should NOT coalesce - creates a new event
    buffer.update(cx, |buffer, cx| {
        let offset = Point::new(25, 0).to_offset(buffer);
        buffer.edit(vec![(offset..offset, "Far below\n")], None, cx);
    });

    let events = ep_store.update(cx, |ep_store, cx| {
        ep_store.edit_history_for_project(&project, cx)
    });
    assert_eq!(
        render_events(&events),
        indoc! {"
            @@ -3,15 +3,16 @@
             Line 3
             Line 4
             Line 5
            +Above
             Line 6
             Line 7
             Line 8
             Line 9
             Line 10
            -Line 11
            -Line 12
            -Line 13
            +Middle A
            +Middle B
             Line 14
            +Below
             Line 15
             Line 16
             Line 17

            ---
            @@ -23,6 +23,7 @@
             Line 22
             Line 23
             Line 24
            +Far below
             Line 25
             Line 26
             Line 27
        "},
        "After inserting far below (should NOT coalesce)"
    );
}

fn render_events(events: &[StoredEvent]) -> String {
    events
        .iter()
        .map(|e| {
            let zeta_prompt::Event::BufferChange { diff, .. } = e.event.as_ref();
            diff.as_str()
        })
        .collect::<Vec<_>>()
        .join("\n---\n")
}

fn render_events_with_predicted(events: &[StoredEvent]) -> Vec<String> {
    events
        .iter()
        .map(|e| {
            let zeta_prompt::Event::BufferChange {
                diff, predicted, ..
            } = e.event.as_ref();
            let prefix = if *predicted { "predicted" } else { "manual" };
            format!("{}\n{}", prefix, diff)
        })
        .collect()
}

fn make_collaborator_replica(
    buffer: &Entity<Buffer>,
    cx: &mut TestAppContext,
) -> (Entity<Buffer>, clock::Global) {
    let (state, version) =
        buffer.read_with(cx, |buffer, _cx| (buffer.to_proto(_cx), buffer.version()));
    let collaborator = cx.new(|_cx| {
        Buffer::from_proto(ReplicaId::new(1), Capability::ReadWrite, state, None).unwrap()
    });
    (collaborator, version)
}

async fn apply_collaborator_edit(
    collaborator: &Entity<Buffer>,
    buffer: &Entity<Buffer>,
    since_version: &mut clock::Global,
    edit_range: Range<usize>,
    new_text: &str,
    cx: &mut TestAppContext,
) {
    collaborator.update(cx, |collaborator, cx| {
        collaborator.edit([(edit_range, new_text)], None, cx);
    });

    let serialize_task = collaborator.read_with(cx, |collaborator, cx| {
        collaborator.serialize_ops(Some(since_version.clone()), cx)
    });
    let ops = serialize_task.await;
    *since_version = collaborator.read_with(cx, |collaborator, _cx| collaborator.version());

    buffer.update(cx, |buffer, cx| {
        buffer.apply_ops(
            ops.into_iter()
                .map(|op| language::proto::deserialize_operation(op).unwrap()),
            cx,
        );
    });
}

#[gpui::test]
async fn test_nearby_collaborator_edits_are_kept_in_history(cx: &mut TestAppContext) {
    let (ep_store, _requests) = init_test_with_fake_client(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        "/root",
        json!({
            "foo.rs": "line 0\nline 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\nline 9\nline 10\nline 11\nline 12\nline 13\nline 14\n"
        }),
    )
    .await;
    let project = Project::test(fs, vec![path!("/root").as_ref()], cx).await;

    let buffer = project
        .update(cx, |project, cx| {
            let path = project.find_project_path(path!("root/foo.rs"), cx).unwrap();
            project.set_active_path(Some(path.clone()), cx);
            project.open_buffer(path, cx)
        })
        .await
        .unwrap();

    let cursor = buffer.read_with(cx, |buffer, _cx| buffer.anchor_before(Point::new(1, 0)));

    ep_store.update(cx, |ep_store, cx| {
        ep_store.register_buffer(&buffer, &project, cx);
        let _ = ep_store.prediction_at(&buffer, Some(cursor), &project, cx);
    });

    buffer.update(cx, |buffer, cx| {
        buffer.edit(vec![(0..6, "LOCAL ZERO")], None, cx);
    });

    let (collaborator, mut collaborator_version) = make_collaborator_replica(&buffer, cx);

    let (line_one_start, line_one_len) = collaborator.read_with(cx, |buffer, _cx| {
        (Point::new(1, 0).to_offset(buffer), buffer.line_len(1))
    });

    apply_collaborator_edit(
        &collaborator,
        &buffer,
        &mut collaborator_version,
        line_one_start..line_one_start + line_one_len as usize,
        "REMOTE ONE",
        cx,
    )
    .await;

    let events = ep_store.update(cx, |ep_store, cx| {
        ep_store.edit_history_for_project(&project, cx)
    });

    assert_eq!(
        render_events_with_predicted(&events),
        vec![indoc! {"
            manual
            @@ -1,5 +1,5 @@
            -line 0
            -line 1
            +LOCAL ZERO
            +REMOTE ONE
             line 2
             line 3
             line 4
        "}]
    );
}

#[gpui::test]
async fn test_distant_collaborator_edits_are_omitted_from_history(cx: &mut TestAppContext) {
    let (ep_store, _requests) = init_test_with_fake_client(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        "/root",
        json!({
            "foo.rs": (0..1000)
                .map(|i| format!("line {i}\n"))
                .collect::<String>()
        }),
    )
    .await;
    let project = Project::test(fs, vec![path!("/root").as_ref()], cx).await;

    let buffer = project
        .update(cx, |project, cx| {
            let path = project.find_project_path(path!("root/foo.rs"), cx).unwrap();
            project.set_active_path(Some(path.clone()), cx);
            project.open_buffer(path, cx)
        })
        .await
        .unwrap();

    let cursor = buffer.read_with(cx, |buffer, _cx| buffer.anchor_before(Point::new(1, 0)));

    ep_store.update(cx, |ep_store, cx| {
        ep_store.register_buffer(&buffer, &project, cx);
        let _ = ep_store.prediction_at(&buffer, Some(cursor), &project, cx);
    });

    buffer.update(cx, |buffer, cx| {
        buffer.edit(vec![(0..6, "LOCAL ZERO")], None, cx);
    });

    let (collaborator, mut collaborator_version) = make_collaborator_replica(&buffer, cx);

    let far_line_start = buffer.read_with(cx, |buffer, _cx| Point::new(900, 0).to_offset(buffer));

    apply_collaborator_edit(
        &collaborator,
        &buffer,
        &mut collaborator_version,
        far_line_start..far_line_start + 7,
        "REMOTE FAR",
        cx,
    )
    .await;

    let events = ep_store.update(cx, |ep_store, cx| {
        ep_store.edit_history_for_project(&project, cx)
    });

    assert_eq!(
        render_events_with_predicted(&events),
        vec![indoc! {"
            manual
            @@ -1,4 +1,4 @@
            -line 0
            +LOCAL ZERO
             line 1
             line 2
             line 3
        "}]
    );
}

#[gpui::test]
async fn test_irrelevant_collaborator_edits_in_different_files_are_omitted_from_history(
    cx: &mut TestAppContext,
) {
    let (ep_store, _requests) = init_test_with_fake_client(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        "/root",
        json!({
            "foo.rs": "line 0\nline 1\nline 2\nline 3\n",
            "bar.rs": "line 0\nline 1\nline 2\nline 3\n"
        }),
    )
    .await;
    let project = Project::test(fs, vec![path!("/root").as_ref()], cx).await;

    let foo_buffer = project
        .update(cx, |project, cx| {
            let path = project.find_project_path(path!("root/foo.rs"), cx).unwrap();
            project.set_active_path(Some(path.clone()), cx);
            project.open_buffer(path, cx)
        })
        .await
        .unwrap();
    let bar_buffer = project
        .update(cx, |project, cx| {
            let path = project.find_project_path(path!("root/bar.rs"), cx).unwrap();
            project.open_buffer(path, cx)
        })
        .await
        .unwrap();

    let foo_cursor = foo_buffer.read_with(cx, |buffer, _cx| buffer.anchor_before(Point::new(1, 0)));

    ep_store.update(cx, |ep_store, cx| {
        ep_store.register_buffer(&foo_buffer, &project, cx);
        ep_store.register_buffer(&bar_buffer, &project, cx);
        let _ = ep_store.prediction_at(&foo_buffer, Some(foo_cursor), &project, cx);
    });

    let (bar_collaborator, mut bar_version) = make_collaborator_replica(&bar_buffer, cx);

    apply_collaborator_edit(
        &bar_collaborator,
        &bar_buffer,
        &mut bar_version,
        0..6,
        "REMOTE BAR",
        cx,
    )
    .await;

    let events = ep_store.update(cx, |ep_store, cx| {
        ep_store.edit_history_for_project(&project, cx)
    });

    assert!(events.is_empty());
}

#[gpui::test]
async fn test_large_edits_are_omitted_from_history(cx: &mut TestAppContext) {
    let (ep_store, _requests) = init_test_with_fake_client(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        "/root",
        json!({
            "foo.rs": (0..20)
                .map(|i| format!("line {i}\n"))
                .collect::<String>()
        }),
    )
    .await;
    let project = Project::test(fs, vec![path!("/root").as_ref()], cx).await;

    let buffer = project
        .update(cx, |project, cx| {
            let path = project.find_project_path(path!("root/foo.rs"), cx).unwrap();
            project.set_active_path(Some(path.clone()), cx);
            project.open_buffer(path, cx)
        })
        .await
        .unwrap();

    let cursor = buffer.read_with(cx, |buffer, _cx| buffer.anchor_before(Point::new(1, 0)));

    ep_store.update(cx, |ep_store, cx| {
        ep_store.register_buffer(&buffer, &project, cx);
        let _ = ep_store.prediction_at(&buffer, Some(cursor), &project, cx);
    });

    buffer.update(cx, |buffer, cx| {
        buffer.edit(vec![(0..6, "LOCAL ZERO")], None, cx);
    });

    let (collaborator, mut collaborator_version) = make_collaborator_replica(&buffer, cx);

    let (line_three_start, line_three_len) = collaborator.read_with(cx, |buffer, _cx| {
        (Point::new(3, 0).to_offset(buffer), buffer.line_len(3))
    });
    let large_edit = "X".repeat(EDIT_HISTORY_DIFF_SIZE_LIMIT + 1);

    apply_collaborator_edit(
        &collaborator,
        &buffer,
        &mut collaborator_version,
        line_three_start..line_three_start + line_three_len as usize,
        &large_edit,
        cx,
    )
    .await;

    buffer.update(cx, |buffer, cx| {
        let line_seven_start = Point::new(7, 0).to_offset(buffer);
        let line_seven_end = Point::new(7, 6).to_offset(buffer);
        buffer.edit(
            vec![(line_seven_start..line_seven_end, "LOCAL SEVEN")],
            None,
            cx,
        );
    });

    let events = ep_store.update(cx, |ep_store, cx| {
        ep_store.edit_history_for_project(&project, cx)
    });

    let rendered_events = render_events_with_predicted(&events);

    assert_eq!(rendered_events.len(), 2);
    assert!(rendered_events[0].contains("+LOCAL ZERO"));
    assert!(!rendered_events[0].contains(&large_edit));
    assert!(rendered_events[1].contains("+LOCAL SEVEN"));
    assert!(!rendered_events[1].contains(&large_edit));
}

#[gpui::test]
async fn test_predicted_flag_coalescing(cx: &mut TestAppContext) {
    let (ep_store, _requests) = init_test_with_fake_client(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        "/root",
        json!({
            "foo.rs": "line 0\nline 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\nline 9\nline 10\nline 11\nline 12\nline 13\nline 14\n"
        }),
    )
    .await;
    let project = Project::test(fs, vec![path!("/root").as_ref()], cx).await;

    let buffer = project
        .update(cx, |project, cx| {
            let path = project.find_project_path(path!("root/foo.rs"), cx).unwrap();
            project.open_buffer(path, cx)
        })
        .await
        .unwrap();

    ep_store.update(cx, |ep_store, cx| {
        ep_store.register_buffer(&buffer, &project, cx);
    });

    // Case 1: Manual edits have `predicted` set to false.
    buffer.update(cx, |buffer, cx| {
        buffer.edit(vec![(0..6, "LINE ZERO")], None, cx);
    });

    let events = ep_store.update(cx, |ep_store, cx| {
        ep_store.edit_history_for_project(&project, cx)
    });

    assert_eq!(
        render_events_with_predicted(&events),
        vec![indoc! {"
            manual
            @@ -1,4 +1,4 @@
            -line 0
            +LINE ZERO
             line 1
             line 2
             line 3
        "}]
    );

    // Case 2: Multiple successive manual edits near each other are merged into one
    // event with `predicted` set to false.
    buffer.update(cx, |buffer, cx| {
        let offset = Point::new(1, 0).to_offset(buffer);
        let end = Point::new(1, 6).to_offset(buffer);
        buffer.edit(vec![(offset..end, "LINE ONE")], None, cx);
    });

    let events = ep_store.update(cx, |ep_store, cx| {
        ep_store.edit_history_for_project(&project, cx)
    });
    assert_eq!(
        render_events_with_predicted(&events),
        vec![indoc! {"
            manual
            @@ -1,5 +1,5 @@
            -line 0
            -line 1
            +LINE ZERO
            +LINE ONE
             line 2
             line 3
             line 4
        "}]
    );

    // Case 3: Accepted predictions have `predicted` set to true.
    // Case 5: A manual edit that follows a predicted edit is not merged with the
    // predicted edit, even if it is nearby.
    ep_store.update(cx, |ep_store, cx| {
        buffer.update(cx, |buffer, cx| {
            let offset = Point::new(2, 0).to_offset(buffer);
            let end = Point::new(2, 6).to_offset(buffer);
            buffer.edit(vec![(offset..end, "LINE TWO")], None, cx);
        });
        ep_store.report_changes_for_buffer(&buffer, &project, true, true, cx);
    });

    let events = ep_store.update(cx, |ep_store, cx| {
        ep_store.edit_history_for_project(&project, cx)
    });
    assert_eq!(
        render_events_with_predicted(&events),
        vec![
            indoc! {"
                manual
                @@ -1,5 +1,5 @@
                -line 0
                -line 1
                +LINE ZERO
                +LINE ONE
                 line 2
                 line 3
                 line 4
            "},
            indoc! {"
                predicted
                @@ -1,6 +1,6 @@
                 LINE ZERO
                 LINE ONE
                -line 2
                +LINE TWO
                 line 3
                 line 4
                 line 5
            "}
        ]
    );

    // Case 4: Multiple successive accepted predictions near each other are merged
    // into one event with `predicted` set to true.
    ep_store.update(cx, |ep_store, cx| {
        buffer.update(cx, |buffer, cx| {
            let offset = Point::new(3, 0).to_offset(buffer);
            let end = Point::new(3, 6).to_offset(buffer);
            buffer.edit(vec![(offset..end, "LINE THREE")], None, cx);
        });
        ep_store.report_changes_for_buffer(&buffer, &project, true, true, cx);
    });

    let events = ep_store.update(cx, |ep_store, cx| {
        ep_store.edit_history_for_project(&project, cx)
    });
    assert_eq!(
        render_events_with_predicted(&events),
        vec![
            indoc! {"
                manual
                @@ -1,5 +1,5 @@
                -line 0
                -line 1
                +LINE ZERO
                +LINE ONE
                 line 2
                 line 3
                 line 4
            "},
            indoc! {"
                predicted
                @@ -1,7 +1,7 @@
                 LINE ZERO
                 LINE ONE
                -line 2
                -line 3
                +LINE TWO
                +LINE THREE
                 line 4
                 line 5
                 line 6
            "}
        ]
    );

    // Case 5 (continued): A manual edit that follows a predicted edit is not merged
    // with the predicted edit, even if it is nearby.
    buffer.update(cx, |buffer, cx| {
        let offset = Point::new(4, 0).to_offset(buffer);
        let end = Point::new(4, 6).to_offset(buffer);
        buffer.edit(vec![(offset..end, "LINE FOUR")], None, cx);
    });

    let events = ep_store.update(cx, |ep_store, cx| {
        ep_store.edit_history_for_project(&project, cx)
    });
    assert_eq!(
        render_events_with_predicted(&events),
        vec![
            indoc! {"
                manual
                @@ -1,5 +1,5 @@
                -line 0
                -line 1
                +LINE ZERO
                +LINE ONE
                 line 2
                 line 3
                 line 4
            "},
            indoc! {"
                predicted
                @@ -1,7 +1,7 @@
                 LINE ZERO
                 LINE ONE
                -line 2
                -line 3
                +LINE TWO
                +LINE THREE
                 line 4
                 line 5
                 line 6
            "},
            indoc! {"
                manual
                @@ -2,7 +2,7 @@
                 LINE ONE
                 LINE TWO
                 LINE THREE
                -line 4
                +LINE FOUR
                 line 5
                 line 6
                 line 7
            "}
        ]
    );

    // Case 6: If we then perform a manual edit at a *different* location (more than
    // 8 lines away), then the edits at the prior location can be merged with each
    // other, even if some are predicted and some are not. `predicted` means all
    // constituent edits were predicted.
    buffer.update(cx, |buffer, cx| {
        let offset = Point::new(14, 0).to_offset(buffer);
        let end = Point::new(14, 7).to_offset(buffer);
        buffer.edit(vec![(offset..end, "LINE FOURTEEN")], None, cx);
    });

    let events = ep_store.update(cx, |ep_store, cx| {
        ep_store.edit_history_for_project(&project, cx)
    });
    assert_eq!(
        render_events_with_predicted(&events),
        vec![
            indoc! {"
                manual
                @@ -1,8 +1,8 @@
                -line 0
                -line 1
                -line 2
                -line 3
                -line 4
                +LINE ZERO
                +LINE ONE
                +LINE TWO
                +LINE THREE
                +LINE FOUR
                 line 5
                 line 6
                 line 7
            "},
            indoc! {"
                manual
                @@ -12,4 +12,4 @@
                 line 11
                 line 12
                 line 13
                -line 14
                +LINE FOURTEEN
            "}
        ]
    );
}

#[gpui::test]
async fn test_empty_prediction(cx: &mut TestAppContext) {
    let (ep_store, mut requests) = init_test_with_fake_client(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        "/root",
        json!({
            "foo.md":  "Hello!\nHow\nBye\n"
        }),
    )
    .await;
    let project = Project::test(fs, vec![path!("/root").as_ref()], cx).await;

    let buffer = project
        .update(cx, |project, cx| {
            let path = project.find_project_path(path!("root/foo.md"), cx).unwrap();
            project.open_buffer(path, cx)
        })
        .await
        .unwrap();
    let snapshot = buffer.read_with(cx, |buffer, _cx| buffer.snapshot());
    let position = snapshot.anchor_before(language::Point::new(1, 3));

    ep_store.update(cx, |ep_store, cx| {
        ep_store.register_buffer(&buffer, &project, cx);
        ep_store.refresh_prediction_from_buffer(
            project.clone(),
            buffer.clone(),
            position,
            Duration::ZERO,
            EditPredictionRequestTrigger::Explicit,
            cx,
        );
    });

    let (request, respond_tx) = requests.predict.next().await.unwrap();
    respond_tx.send(model_response(&request, "")).unwrap();

    cx.run_until_parked();

    ep_store.update(cx, |ep_store, cx| {
        assert!(
            ep_store
                .prediction_at(&buffer, None, &project, cx)
                .is_none()
        );
    });
}

#[gpui::test]
async fn test_interpolated_empty(cx: &mut TestAppContext) {
    let (ep_store, mut requests) = init_test_with_fake_client(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        "/root",
        json!({
            "foo.md":  "Hello!\nHow\nBye\n"
        }),
    )
    .await;
    let project = Project::test(fs, vec![path!("/root").as_ref()], cx).await;

    let buffer = project
        .update(cx, |project, cx| {
            let path = project.find_project_path(path!("root/foo.md"), cx).unwrap();
            project.open_buffer(path, cx)
        })
        .await
        .unwrap();
    let snapshot = buffer.read_with(cx, |buffer, _cx| buffer.snapshot());
    let position = snapshot.anchor_before(language::Point::new(1, 3));

    ep_store.update(cx, |ep_store, cx| {
        ep_store.refresh_prediction_from_buffer(
            project.clone(),
            buffer.clone(),
            position,
            Duration::ZERO,
            EditPredictionRequestTrigger::Other,
            cx,
        );
    });

    let (request, respond_tx) = requests.predict.next().await.unwrap();

    buffer.update(cx, |buffer, cx| {
        buffer.edit([(10..10, " are you?")], None, cx);
    });

    respond_tx
        .send(model_response(&request, SIMPLE_DIFF))
        .unwrap();

    cx.run_until_parked();

    ep_store.update(cx, |ep_store, cx| {
        assert!(
            ep_store
                .prediction_at(&buffer, None, &project, cx)
                .is_none()
        );
    });
}

#[gpui::test]
async fn test_interpolate_failed(cx: &mut TestAppContext) {
    let (ep_store, mut requests) = init_test_with_fake_client(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        "/root",
        json!({
            "foo.md":  "Hello!\nHow\nBye\n"
        }),
    )
    .await;
    let project = Project::test(fs, vec![path!("/root").as_ref()], cx).await;

    let buffer = project
        .update(cx, |project, cx| {
            let path = project.find_project_path(path!("root/foo.md"), cx).unwrap();
            project.open_buffer(path, cx)
        })
        .await
        .unwrap();
    let snapshot = buffer.read_with(cx, |buffer, _cx| buffer.snapshot());
    let position = snapshot.anchor_before(language::Point::new(1, 3));

    ep_store.update(cx, |ep_store, cx| {
        ep_store.refresh_prediction_from_buffer(
            project.clone(),
            buffer.clone(),
            position,
            Duration::ZERO,
            EditPredictionRequestTrigger::Other,
            cx,
        );
    });

    let (request, respond_tx) = requests.predict.next().await.unwrap();

    buffer.update(cx, |buffer, cx| {
        buffer.edit([(10..10, " is it?")], None, cx);
    });

    respond_tx
        .send(model_response(&request, SIMPLE_DIFF))
        .unwrap();

    cx.run_until_parked();

    ep_store.update(cx, |ep_store, cx| {
        assert!(
            ep_store
                .prediction_at(&buffer, None, &project, cx)
                .is_none()
        );
    });
}

const SIMPLE_DIFF: &str = indoc! { r"
    --- a/root/foo.md
    +++ b/root/foo.md
    @@ ... @@
     Hello!
    -How
    +How are you?
     Bye
"};

#[gpui::test]
async fn test_replace_current(cx: &mut TestAppContext) {
    let (ep_store, mut requests) = init_test_with_fake_client(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        "/root",
        json!({
            "foo.md":  "Hello!\nHow\nBye\n"
        }),
    )
    .await;
    let project = Project::test(fs, vec![path!("/root").as_ref()], cx).await;

    let buffer = project
        .update(cx, |project, cx| {
            let path = project.find_project_path(path!("root/foo.md"), cx).unwrap();
            project.open_buffer(path, cx)
        })
        .await
        .unwrap();
    let snapshot = buffer.read_with(cx, |buffer, _cx| buffer.snapshot());
    let position = snapshot.anchor_before(language::Point::new(1, 3));

    ep_store.update(cx, |ep_store, cx| {
        ep_store.refresh_prediction_from_buffer(
            project.clone(),
            buffer.clone(),
            position,
            Duration::ZERO,
            EditPredictionRequestTrigger::Other,
            cx,
        );
    });

    let (request, respond_tx) = requests.predict.next().await.unwrap();
    let first_response = model_response(&request, SIMPLE_DIFF);
    let first_id = first_response.request_id.clone();
    respond_tx.send(first_response).unwrap();

    cx.run_until_parked();

    ep_store.update(cx, |ep_store, cx| {
        assert_eq!(
            ep_store
                .prediction_at(&buffer, None, &project, cx)
                .unwrap()
                .id
                .0,
            first_id
        );
    });

    // a second request is triggered
    ep_store.update(cx, |ep_store, cx| {
        ep_store.refresh_prediction_from_buffer(
            project.clone(),
            buffer.clone(),
            position,
            Duration::ZERO,
            EditPredictionRequestTrigger::Other,
            cx,
        );
    });

    let (request, respond_tx) = requests.predict.next().await.unwrap();
    let second_response = model_response(&request, SIMPLE_DIFF);
    let second_id = second_response.request_id.clone();
    respond_tx.send(second_response).unwrap();

    cx.run_until_parked();

    ep_store.update(cx, |ep_store, cx| {
        // second replaces first
        assert_eq!(
            ep_store
                .prediction_at(&buffer, None, &project, cx)
                .unwrap()
                .id
                .0,
            second_id
        );
    });
}

#[gpui::test]
async fn test_current_preferred(cx: &mut TestAppContext) {
    let (ep_store, mut requests) = init_test_with_fake_client(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        "/root",
        json!({
            "foo.md":  "Hello!\nHow\nBye\n"
        }),
    )
    .await;
    let project = Project::test(fs, vec![path!("/root").as_ref()], cx).await;

    let buffer = project
        .update(cx, |project, cx| {
            let path = project.find_project_path(path!("root/foo.md"), cx).unwrap();
            project.open_buffer(path, cx)
        })
        .await
        .unwrap();
    let snapshot = buffer.read_with(cx, |buffer, _cx| buffer.snapshot());
    let position = snapshot.anchor_before(language::Point::new(1, 3));

    ep_store.update(cx, |ep_store, cx| {
        ep_store.refresh_prediction_from_buffer(
            project.clone(),
            buffer.clone(),
            position,
            Duration::ZERO,
            EditPredictionRequestTrigger::Other,
            cx,
        );
    });

    let (request, respond_tx) = requests.predict.next().await.unwrap();
    let first_response = model_response(&request, SIMPLE_DIFF);
    let first_id = first_response.request_id.clone();
    respond_tx.send(first_response).unwrap();

    cx.run_until_parked();

    ep_store.update(cx, |ep_store, cx| {
        assert_eq!(
            ep_store
                .prediction_at(&buffer, None, &project, cx)
                .unwrap()
                .id
                .0,
            first_id
        );
    });

    // a second request is triggered
    ep_store.update(cx, |ep_store, cx| {
        ep_store.refresh_prediction_from_buffer(
            project.clone(),
            buffer.clone(),
            position,
            Duration::ZERO,
            EditPredictionRequestTrigger::Other,
            cx,
        );
    });

    let (request, respond_tx) = requests.predict.next().await.unwrap();
    // worse than current prediction
    let second_response = model_response(
        &request,
        indoc! { r"
            --- a/root/foo.md
            +++ b/root/foo.md
            @@ ... @@
             Hello!
            -How
            +How are
             Bye
        "},
    );
    respond_tx.send(second_response).unwrap();

    cx.run_until_parked();

    ep_store.update(cx, |ep_store, cx| {
        // first is preferred over second
        assert_eq!(
            ep_store
                .prediction_at(&buffer, None, &project, cx)
                .unwrap()
                .id
                .0,
            first_id
        );
    });
}

#[gpui::test]
async fn test_cancel_earlier_pending_requests(cx: &mut TestAppContext) {
    let (ep_store, mut requests) = init_test_with_fake_client(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        "/root",
        json!({
            "foo.md":  "Hello!\nHow\nBye\n"
        }),
    )
    .await;
    let project = Project::test(fs, vec![path!("/root").as_ref()], cx).await;

    let buffer = project
        .update(cx, |project, cx| {
            let path = project.find_project_path(path!("root/foo.md"), cx).unwrap();
            project.open_buffer(path, cx)
        })
        .await
        .unwrap();
    let snapshot = buffer.read_with(cx, |buffer, _cx| buffer.snapshot());
    let position = snapshot.anchor_before(language::Point::new(1, 3));

    // start two refresh tasks
    ep_store.update(cx, |ep_store, cx| {
        ep_store.refresh_prediction_from_buffer(
            project.clone(),
            buffer.clone(),
            position,
            Duration::ZERO,
            EditPredictionRequestTrigger::Other,
            cx,
        );
    });

    let (request1, respond_first) = requests.predict.next().await.unwrap();

    ep_store.update(cx, |ep_store, cx| {
        ep_store.refresh_prediction_from_buffer(
            project.clone(),
            buffer.clone(),
            position,
            Duration::ZERO,
            EditPredictionRequestTrigger::Other,
            cx,
        );
    });

    let (request, respond_second) = requests.predict.next().await.unwrap();

    // wait for throttle
    cx.run_until_parked();

    // second responds first
    let second_response = model_response(&request, SIMPLE_DIFF);
    let second_id = second_response.request_id.clone();
    respond_second.send(second_response).unwrap();

    cx.run_until_parked();

    ep_store.update(cx, |ep_store, cx| {
        // current prediction is second
        assert_eq!(
            ep_store
                .prediction_at(&buffer, None, &project, cx)
                .unwrap()
                .id
                .0,
            second_id
        );
    });

    // The first request's task was dropped when it was superseded, so there
    // is nobody left to receive its response.
    respond_first
        .send(model_response(&request1, SIMPLE_DIFF))
        .ok();

    cx.run_until_parked();

    ep_store.update(cx, |ep_store, cx| {
        // current prediction is still second, since first was cancelled
        assert_eq!(
            ep_store
                .prediction_at(&buffer, None, &project, cx)
                .unwrap()
                .id
                .0,
            second_id
        );
    });
}

#[gpui::test]
async fn test_cancel_second_on_third_request(cx: &mut TestAppContext) {
    let (ep_store, mut requests) = init_test_with_fake_client(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        "/root",
        json!({
            "foo.md":  "Hello!\nHow\nBye\n"
        }),
    )
    .await;
    let project = Project::test(fs, vec![path!("/root").as_ref()], cx).await;

    let buffer = project
        .update(cx, |project, cx| {
            let path = project.find_project_path(path!("root/foo.md"), cx).unwrap();
            project.open_buffer(path, cx)
        })
        .await
        .unwrap();
    let snapshot = buffer.read_with(cx, |buffer, _cx| buffer.snapshot());
    let position = snapshot.anchor_before(language::Point::new(1, 3));

    // start two refresh tasks
    ep_store.update(cx, |ep_store, cx| {
        ep_store.refresh_prediction_from_buffer(
            project.clone(),
            buffer.clone(),
            position,
            Duration::ZERO,
            EditPredictionRequestTrigger::Other,
            cx,
        );
    });

    let (request1, respond_first) = requests.predict.next().await.unwrap();

    ep_store.update(cx, |ep_store, cx| {
        ep_store.refresh_prediction_from_buffer(
            project.clone(),
            buffer.clone(),
            position,
            Duration::ZERO,
            EditPredictionRequestTrigger::Other,
            cx,
        );
    });

    let (request2, respond_second) = requests.predict.next().await.unwrap();

    // wait for throttle, so requests are sent
    cx.run_until_parked();

    ep_store.update(cx, |ep_store, cx| {
        // start a third request
        ep_store.refresh_prediction_from_buffer(
            project.clone(),
            buffer.clone(),
            position,
            Duration::ZERO,
            EditPredictionRequestTrigger::Other,
            cx,
        );

        // 2 are pending, so 2nd is cancelled
        assert_eq!(
            ep_store
                .get_or_init_project(&project, cx)
                .cancelled_predictions
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            [1]
        );
    });

    // wait for throttle
    cx.run_until_parked();

    let (request3, respond_third) = requests.predict.next().await.unwrap();

    let first_response = model_response(&request1, SIMPLE_DIFF);
    let first_id = first_response.request_id.clone();
    respond_first.send(first_response).unwrap();

    cx.run_until_parked();

    ep_store.update(cx, |ep_store, cx| {
        // current prediction is first
        assert_eq!(
            ep_store
                .prediction_at(&buffer, None, &project, cx)
                .unwrap()
                .id
                .0,
            first_id
        );
    });

    respond_second
        .send(model_response(&request2, SIMPLE_DIFF))
        .ok();

    cx.run_until_parked();

    ep_store.update(cx, |ep_store, cx| {
        // current prediction is still first, since second was cancelled
        assert_eq!(
            ep_store
                .prediction_at(&buffer, None, &project, cx)
                .unwrap()
                .id
                .0,
            first_id
        );
    });

    let third_response = model_response(&request3, SIMPLE_DIFF);
    let third_response_id = third_response.request_id.clone();
    respond_third.send(third_response).unwrap();

    cx.run_until_parked();

    ep_store.update(cx, |ep_store, cx| {
        // third completes and replaces first
        assert_eq!(
            ep_store
                .prediction_at(&buffer, None, &project, cx)
                .unwrap()
                .id
                .0,
            third_response_id
        );
    });
}

#[gpui::test]
async fn test_same_frame_duplicate_requests_deduplicated(cx: &mut TestAppContext) {
    let (ep_store, mut requests) = init_test_with_fake_client(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        "/root",
        json!({
            "foo.md":  "Hello!\nHow\nBye\n"
        }),
    )
    .await;
    let project = Project::test(fs, vec![path!("/root").as_ref()], cx).await;

    let buffer = project
        .update(cx, |project, cx| {
            let path = project.find_project_path(path!("root/foo.md"), cx).unwrap();
            project.open_buffer(path, cx)
        })
        .await
        .unwrap();
    let snapshot = buffer.read_with(cx, |buffer, _cx| buffer.snapshot());
    let position = snapshot.anchor_before(language::Point::new(1, 3));

    // Enqueue two refresh calls in the same synchronous frame (no yielding).
    // Both `cx.spawn` tasks are created before either executes, so they both
    // capture the same `proceed_count_at_enqueue`. Only the first task should
    // pass the deduplication gate; the second should be skipped.
    ep_store.update(cx, |ep_store, cx| {
        ep_store.refresh_prediction_from_buffer(
            project.clone(),
            buffer.clone(),
            position,
            Duration::ZERO,
            EditPredictionRequestTrigger::Other,
            cx,
        );
        ep_store.refresh_prediction_from_buffer(
            project.clone(),
            buffer.clone(),
            position,
            Duration::ZERO,
            EditPredictionRequestTrigger::Other,
            cx,
        );
    });

    // Let both spawned tasks run to completion (including any throttle waits).
    cx.run_until_parked();

    // Exactly one prediction request should have been sent.
    let (request, respond_tx) = requests.predict.next().await.unwrap();
    respond_tx
        .send(model_response(&request, SIMPLE_DIFF))
        .unwrap();
    cx.run_until_parked();

    // No second request should be pending.
    assert_no_predict_request_ready(&mut requests.predict);
}

#[gpui::test]
fn test_active_buffer_diagnostics_fetching(cx: &mut TestAppContext) {
    let diagnostic_marker: TextRangeMarker = ('«', '»').into();
    let search_range_marker: TextRangeMarker = ('[', ']').into();

    let (text, mut ranges) = marked_text_ranges_by(
        indoc! {r#"
            fn alpha() {
                let «first_value» = 1;
            }

            [fn beta() {
                let «second_value» = 2;
                let third_value = second_value + missing_symbol;
            }ˇ]

            fn gamma() {
                let «fourth_value» = missing_other_symbol;
            }
        "#},
        vec![diagnostic_marker.clone(), search_range_marker.clone()],
    );

    let diagnostic_ranges = ranges.remove(&diagnostic_marker).unwrap_or_default();
    let search_ranges = ranges.remove(&search_range_marker).unwrap_or_default();

    let buffer = cx.new(|cx| Buffer::local(&text, cx));

    buffer.update(cx, |buffer, cx| {
        let snapshot = buffer.snapshot();
        let diagnostics = DiagnosticSet::new(
            diagnostic_ranges.iter().enumerate().map(|(index, range)| {
                DiagnosticEntry::new(
                    snapshot.offset_to_point_utf16(range.start)
                        ..snapshot.offset_to_point_utf16(range.end),
                    Diagnostic {
                        severity: match index {
                            0 => DiagnosticSeverity::WARNING,
                            1 => DiagnosticSeverity::ERROR,
                            _ => DiagnosticSeverity::HINT,
                        },
                        message: match index {
                            0 => "first warning".to_string(),
                            1 => "second error".to_string(),
                            _ => "third hint".to_string(),
                        },
                        group_id: index + 1,
                        is_primary: true,
                        source_kind: language::DiagnosticSourceKind::Pushed,
                        ..Diagnostic::default()
                    },
                )
            }),
            &snapshot,
        );
        buffer.update_diagnostics(LanguageServerId(0), diagnostics, cx);
    });

    let snapshot = buffer.read_with(cx, |buffer, _cx| buffer.snapshot());
    let search_range = snapshot.offset_to_point(search_ranges[0].start)
        ..snapshot.offset_to_point(search_ranges[0].end);

    let active_buffer_diagnostics = zeta::active_buffer_diagnostics(&snapshot, search_range, 5, 0);

    assert_eq!(
        active_buffer_diagnostics,
        vec![zeta_prompt::ActiveBufferDiagnostic {
            severity: Some(1),
            message: "second error".to_string(),
            snippet: "    let second_value = 2;".to_string(),
            snippet_buffer_row_range: 5..5,
            diagnostic_range_in_snippet: 8..20,
        }]
    );

    let active_buffer_diagnostics =
        zeta::active_buffer_diagnostics(&snapshot, Point::new(0, 0)..snapshot.max_point(), 5, 100);
    assert_eq!(
        active_buffer_diagnostics,
        vec![
            zeta_prompt::ActiveBufferDiagnostic {
                severity: Some(1),
                message: "second error".to_string(),
                snippet: String::new(),
                snippet_buffer_row_range: 5..5,
                diagnostic_range_in_snippet: 0..0,
            },
            zeta_prompt::ActiveBufferDiagnostic {
                severity: Some(2),
                message: "first warning".to_string(),
                snippet: String::new(),
                snippet_buffer_row_range: 1..1,
                diagnostic_range_in_snippet: 0..0,
            },
            zeta_prompt::ActiveBufferDiagnostic {
                severity: Some(4),
                message: "third hint".to_string(),
                snippet: String::new(),
                snippet_buffer_row_range: 10..10,
                diagnostic_range_in_snippet: 0..0,
            },
        ]
    );

    let buffer = cx.new(|cx| {
        Buffer::local(
            indoc! {"
                one
                two
                three
                four
                five
            "},
            cx,
        )
    });

    buffer.update(cx, |buffer, cx| {
        let snapshot = buffer.snapshot();
        let diagnostics = DiagnosticSet::new(
            vec![
                DiagnosticEntry::new(
                    text::PointUtf16::new(0, 0)..text::PointUtf16::new(0, 3),
                    Diagnostic {
                        severity: DiagnosticSeverity::ERROR,
                        message: "row zero".to_string(),
                        group_id: 1,
                        is_primary: true,
                        source_kind: language::DiagnosticSourceKind::Pushed,
                        ..Diagnostic::default()
                    },
                ),
                DiagnosticEntry::new(
                    text::PointUtf16::new(2, 0)..text::PointUtf16::new(2, 5),
                    Diagnostic {
                        severity: DiagnosticSeverity::WARNING,
                        message: "row two".to_string(),
                        group_id: 2,
                        is_primary: true,
                        source_kind: language::DiagnosticSourceKind::Pushed,
                        ..Diagnostic::default()
                    },
                ),
                DiagnosticEntry::new(
                    text::PointUtf16::new(4, 0)..text::PointUtf16::new(4, 4),
                    Diagnostic {
                        severity: DiagnosticSeverity::INFORMATION,
                        message: "row four".to_string(),
                        group_id: 3,
                        is_primary: true,
                        source_kind: language::DiagnosticSourceKind::Pushed,
                        ..Diagnostic::default()
                    },
                ),
            ],
            &snapshot,
        );
        buffer.update_diagnostics(LanguageServerId(0), diagnostics, cx);
    });

    let snapshot = buffer.read_with(cx, |buffer, _cx| buffer.snapshot());

    let active_buffer_diagnostics =
        zeta::active_buffer_diagnostics(&snapshot, Point::new(2, 0)..Point::new(4, 0), 3, 0);

    assert_eq!(
        active_buffer_diagnostics
            .iter()
            .map(|diagnostic| (
                diagnostic.severity,
                diagnostic.message.clone(),
                diagnostic.snippet.clone(),
                diagnostic.snippet_buffer_row_range.clone(),
                diagnostic.diagnostic_range_in_snippet.clone(),
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                Some(2),
                "row two".to_string(),
                "three".to_string(),
                2..2,
                0..5,
            ),
            (
                Some(3),
                "row four".to_string(),
                "five".to_string(),
                4..4,
                0..4,
            ),
        ]
    );
}

#[gpui::test]
fn test_active_buffer_diagnostics_collection_limits(cx: &mut TestAppContext) {
    let text = (0..25)
        .map(|row| format!("line {row}\n"))
        .collect::<String>();
    let buffer = cx.new(|cx| Buffer::local(&text, cx));

    buffer.update(cx, |buffer, cx| {
        let snapshot = buffer.snapshot();
        let diagnostics = DiagnosticSet::new(
            (0..25)
                .map(|row| {
                    DiagnosticEntry::new(
                        text::PointUtf16::new(row, 0)..text::PointUtf16::new(row, 4),
                        Diagnostic {
                            severity: DiagnosticSeverity::ERROR,
                            message: format!("row {row}"),
                            group_id: row as usize,
                            is_primary: true,
                            source_kind: language::DiagnosticSourceKind::Pushed,
                            ..Diagnostic::default()
                        },
                    )
                })
                .collect::<Vec<_>>(),
            &snapshot,
        );
        buffer.update_diagnostics(LanguageServerId(0), diagnostics, cx);
    });

    let snapshot = buffer.read_with(cx, |buffer, _cx| buffer.snapshot());
    let active_buffer_diagnostics =
        zeta::active_buffer_diagnostics(&snapshot, Point::new(0, 0)..Point::new(25, 0), 12, 0);

    assert_eq!(active_buffer_diagnostics.len(), 20);
    assert!(
        active_buffer_diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message == "row 12")
    );
    assert!(
        active_buffer_diagnostics
            .iter()
            .all(|diagnostic| diagnostic.message != "row 0" && diagnostic.message != "row 24")
    );

    let text = (0..300)
        .map(|row| format!("line {row} has some diagnostic context\n"))
        .collect::<String>();
    let long_message = "diagnostic message ".repeat(1000);
    let buffer = cx.new(|cx| Buffer::local(&text, cx));

    buffer.update(cx, |buffer, cx| {
        let snapshot = buffer.snapshot();
        let diagnostics = DiagnosticSet::new(
            vec![DiagnosticEntry::new(
                text::PointUtf16::new(150, 0)..text::PointUtf16::new(150, 4),
                Diagnostic {
                    severity: DiagnosticSeverity::ERROR,
                    message: long_message.clone(),
                    group_id: 1,
                    is_primary: true,
                    source_kind: language::DiagnosticSourceKind::Pushed,
                    ..Diagnostic::default()
                },
            )],
            &snapshot,
        );
        buffer.update_diagnostics(LanguageServerId(0), diagnostics, cx);
    });

    let snapshot = buffer.read_with(cx, |buffer, _cx| buffer.snapshot());
    let active_buffer_diagnostics = zeta::active_buffer_diagnostics(
        &snapshot,
        Point::new(100, 0)..Point::new(200, 0),
        150,
        2000,
    );

    assert_eq!(active_buffer_diagnostics.len(), 1);
    assert!(
        active_buffer_diagnostics[0].message.len()
            <= crate::zeta::MAX_ACTIVE_BUFFER_DIAGNOSTIC_MESSAGE_TOKENS_TO_COLLECT * 3 + 2
    );
    assert!(active_buffer_diagnostics[0].message.len() < long_message.len());
    assert!(
        active_buffer_diagnostics[0].snippet.len()
            <= crate::zeta::MAX_ACTIVE_BUFFER_DIAGNOSTIC_SNIPPET_TOKENS_TO_COLLECT * 3 + 2
    );
    assert!(active_buffer_diagnostics[0].snippet.len() < text.len());
}

// Generate a model response that would apply the given diff to the active file.
#[gpui::test]
async fn test_edit_prediction_basic_interpolation(cx: &mut TestAppContext) {
    let buffer = cx.new(|cx| Buffer::local("Lorem ipsum dolor", cx));
    let edits: Arc<[(Range<Anchor>, Arc<str>)]> = cx.update(|cx| {
        to_completion_edits([(2..5, "REM".into()), (9..11, "".into())], &buffer, cx).into()
    });

    let edit_preview = cx
        .read(|cx| buffer.read(cx).preview_edits(edits.clone(), cx))
        .await;

    let prediction = EditPrediction {
        edits,
        cursor_position: None,
        editable_range: None,
        edit_preview,
        buffer: buffer.clone(),
        snapshot: cx.read(|cx| buffer.read(cx).snapshot()),
        id: EditPredictionId("the-id".into()),
        inputs: EditPredictionInputs::V2(Zeta2PromptInput {
            events: Default::default(),
            related_files: Default::default(),
            active_buffer_diagnostics: vec![],
            cursor_path: Path::new("").into(),
            cursor_excerpt: "".into(),
            cursor_offset_in_excerpt: 0,
            excerpt_start_row: None,
            excerpt_ranges: Default::default(),
            syntax_ranges: None,
            in_open_source_repo: false,
            can_collect_data: false,
            repo_url: None,
        }),
        model_version: None,
        trigger: PredictEditsRequestTrigger::Other,
    };

    cx.update(|cx| {
        assert_eq!(
            from_completion_edits(
                &prediction.interpolate(&buffer.read(cx).snapshot()).unwrap(),
                &buffer,
                cx
            ),
            vec![(2..5, "REM".into()), (9..11, "".into())]
        );

        buffer.update(cx, |buffer, cx| buffer.edit([(2..5, "")], None, cx));
        assert_eq!(
            from_completion_edits(
                &prediction.interpolate(&buffer.read(cx).snapshot()).unwrap(),
                &buffer,
                cx
            ),
            vec![(2..2, "REM".into()), (6..8, "".into())]
        );

        buffer.update(cx, |buffer, cx| buffer.undo(cx));
        assert_eq!(
            from_completion_edits(
                &prediction.interpolate(&buffer.read(cx).snapshot()).unwrap(),
                &buffer,
                cx
            ),
            vec![(2..5, "REM".into()), (9..11, "".into())]
        );

        buffer.update(cx, |buffer, cx| buffer.edit([(2..5, "R")], None, cx));
        assert_eq!(
            from_completion_edits(
                &prediction.interpolate(&buffer.read(cx).snapshot()).unwrap(),
                &buffer,
                cx
            ),
            vec![(3..3, "EM".into()), (7..9, "".into())]
        );

        buffer.update(cx, |buffer, cx| buffer.edit([(3..3, "E")], None, cx));
        assert_eq!(
            from_completion_edits(
                &prediction.interpolate(&buffer.read(cx).snapshot()).unwrap(),
                &buffer,
                cx
            ),
            vec![(4..4, "M".into()), (8..10, "".into())]
        );

        buffer.update(cx, |buffer, cx| buffer.edit([(4..4, "M")], None, cx));
        assert_eq!(
            from_completion_edits(
                &prediction.interpolate(&buffer.read(cx).snapshot()).unwrap(),
                &buffer,
                cx
            ),
            vec![(9..11, "".into())]
        );

        buffer.update(cx, |buffer, cx| buffer.edit([(4..5, "")], None, cx));
        assert_eq!(
            from_completion_edits(
                &prediction.interpolate(&buffer.read(cx).snapshot()).unwrap(),
                &buffer,
                cx
            ),
            vec![(4..4, "M".into()), (8..10, "".into())]
        );

        buffer.update(cx, |buffer, cx| buffer.edit([(8..10, "")], None, cx));
        assert_eq!(
            from_completion_edits(
                &prediction.interpolate(&buffer.read(cx).snapshot()).unwrap(),
                &buffer,
                cx
            ),
            vec![(4..4, "M".into())]
        );

        buffer.update(cx, |buffer, cx| buffer.edit([(4..6, "")], None, cx));
        assert_eq!(prediction.interpolate(&buffer.read(cx).snapshot()), None);
    })
}

#[gpui::test]
async fn test_clean_up_diff(cx: &mut TestAppContext) {
    init_test(cx);

    assert_eq!(
        apply_edit_prediction(
            indoc! {"
                    fn main() {
                        let word_1 = \"lorem\";
                        let range = word.len()..word.len();
                    }
                "},
            indoc! {"
                    fn main() {
                        let word_1 = \"lorem\";
                        let range = word_1.len()..word_1.len();
                    }
                "},
            cx,
        )
        .await,
        indoc! {"
                fn main() {
                    let word_1 = \"lorem\";
                    let range = word_1.len()..word_1.len();
                }
            "},
    );

    assert_eq!(
        apply_edit_prediction(
            indoc! {"
                    fn main() {
                        let story = \"the quick\"
                    }
                "},
            indoc! {"
                    fn main() {
                        let story = \"the quick brown fox jumps over the lazy dog\";
                    }
                "},
            cx,
        )
        .await,
        indoc! {"
                fn main() {
                    let story = \"the quick brown fox jumps over the lazy dog\";
                }
            "},
    );
}

#[gpui::test]
async fn test_edit_prediction_end_of_buffer(cx: &mut TestAppContext) {
    init_test(cx);

    let buffer_content = "lorem\n";
    let completion_response = "lorem\nipsum\n";

    assert_eq!(
        apply_edit_prediction(buffer_content, completion_response, cx).await,
        "lorem\nipsum\n"
    );
}

#[gpui::test]
async fn test_edit_prediction_no_spurious_trailing_newline(cx: &mut TestAppContext) {
    // Test that zeta2's newline normalization logic doesn't insert spurious newlines.
    // When the buffer ends without a trailing newline, but the model returns output
    // with a trailing newline, zeta2 should normalize both sides before diffing
    // so no spurious newline is inserted.
    let (ep_store, mut requests) = init_test_with_fake_client(cx);
    let fs = FakeFs::new(cx.executor());

    // Single line buffer with no trailing newline
    fs.insert_tree(
        "/root",
        json!({
            "foo.txt": "hello"
        }),
    )
    .await;
    let project = Project::test(fs, vec![path!("/root").as_ref()], cx).await;

    let buffer = project
        .update(cx, |project, cx| {
            let path = project
                .find_project_path(path!("root/foo.txt"), cx)
                .unwrap();
            project.open_buffer(path, cx)
        })
        .await
        .unwrap();

    let snapshot = buffer.read_with(cx, |buffer, _cx| buffer.snapshot());
    let position = snapshot.anchor_before(language::Point::new(0, 5));

    ep_store.update(cx, |ep_store, cx| {
        ep_store.refresh_prediction_from_buffer(
            project.clone(),
            buffer.clone(),
            position,
            Duration::ZERO,
            EditPredictionRequestTrigger::Other,
            cx,
        );
    });

    let (_request, respond_tx) = requests.predict.next().await.unwrap();

    // Model returns output WITH a trailing newline, even though the buffer doesn't have one.
    // Zeta2 should normalize both sides before diffing, so no spurious newline is inserted.
    respond_tx.send(fake_completion("hello world\n")).unwrap();

    cx.run_until_parked();

    // The prediction should insert " world" without adding a newline
    ep_store.update(cx, |ep_store, cx| {
        let prediction = ep_store
            .prediction_at(&buffer, None, &project, cx)
            .expect("should have prediction");
        let edits: Vec<_> = prediction
            .edits
            .iter()
            .map(|(range, text)| {
                let snapshot = buffer.read(cx).snapshot();
                (range.to_offset(&snapshot), text.clone())
            })
            .collect();
        assert_eq!(edits, vec![(5..5, " world".into())]);
    });
}

const FAKE_COMPLETIONS_URL: &str = "http://localhost:8080/v1/completions";

#[derive(Debug)]
struct FakeCompletion {
    request_id: String,
    text: String,
}

fn fake_completion(text: impl Into<String>) -> FakeCompletion {
    FakeCompletion {
        request_id: Uuid::new_v4().to_string(),
        text: text.into(),
    }
}

fn editable_region_from_prompt(prompt: &str) -> String {
    let start_marker = format!("{EDITABLE_REGION_START_MARKER}\n");
    let end_marker = format!("\n{EDITABLE_REGION_END_MARKER}");
    let start = prompt
        .find(&start_marker)
        .expect("prompt should contain the editable region start marker")
        + start_marker.len();
    let length = prompt[start..]
        .find(&end_marker)
        .expect("prompt should contain the editable region end marker");
    prompt[start..start + length].replace(CURSOR_MARKER, "")
}

// Generate a model response that would apply the given diff to the active file.
fn model_response(request: &RawCompletionRequest, diff_to_apply: &str) -> FakeCompletion {
    let excerpt = editable_region_from_prompt(&request.prompt);
    let new_excerpt = apply_diff_to_string(diff_to_apply, &excerpt).unwrap();
    fake_completion(new_excerpt)
}

fn prompt_from_request(request: &RawCompletionRequest) -> String {
    request.prompt.clone()
}

fn assert_no_predict_request_ready<Request, Response>(
    requests: &mut mpsc::UnboundedReceiver<(Request, oneshot::Sender<Response>)>,
) {
    if requests.next().now_or_never().flatten().is_some() {
        panic!("Unexpected prediction request while throttled.");
    }
}

struct RequestChannels {
    predict: mpsc::UnboundedReceiver<(RawCompletionRequest, oneshot::Sender<FakeCompletion>)>,
}

fn configure_fake_zeta_provider(cx: &mut App) {
    SettingsStore::update_global(cx, |store, cx| {
        store.update_user_settings(cx, |settings| {
            settings.project.all_languages.edit_predictions =
                Some(settings::EditPredictionSettingsContent {
                    provider: Some(settings::EditPredictionProvider::OpenAiCompatibleApi),
                    open_ai_compatible_api: Some(
                        settings::CustomEditPredictionProviderSettingsContent {
                            api_url: Some(FAKE_COMPLETIONS_URL.to_string()),
                            model: Some("zeta".to_string()),
                            prompt_format: Some(settings::EditPredictionPromptFormatContent::Zeta),
                            max_output_tokens: Some(64),
                            prediction_debounce: None,
                        },
                    ),
                    ..Default::default()
                });
        });
    });
}

fn completion_http_response(completion: FakeCompletion) -> Response<AsyncBody> {
    let response = RawCompletionResponse {
        id: completion.request_id,
        object: "text_completion".to_string(),
        created: 0,
        model: "zeta".to_string(),
        choices: vec![RawCompletionChoice {
            text: completion.text,
            finish_reason: Some("stop".to_string()),
        }],
        usage: RawCompletionUsage {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
        },
    };
    Response::builder()
        .body(serde_json::to_string(&response).unwrap().into())
        .unwrap()
}

fn init_test_with_fake_client(
    cx: &mut TestAppContext,
) -> (Entity<EditPredictionStore>, RequestChannels) {
    cx.update(|cx| {
        cx.set_global(AppDatabase::test_new());
        let settings_store = SettingsStore::test(cx);
        cx.set_global(settings_store);
        zlog::init_test();
        configure_fake_zeta_provider(cx);

        let (predict_request_tx, predict_request_rx) = mpsc::unbounded();

        let http_client = FakeHttpClient::create(move |request| {
            let predict_request_tx = predict_request_tx.clone();
            async move {
                let mut body = request.into_body();
                let mut buffer = Vec::new();
                body.read_to_end(&mut buffer).await.ok();
                let request: RawCompletionRequest = serde_json::from_slice(&buffer).unwrap();

                let (response_tx, response_rx) = oneshot::channel::<FakeCompletion>();
                predict_request_tx
                    .unbounded_send((request, response_tx))
                    .unwrap();
                Ok(completion_http_response(response_rx.await?))
            }
        });
        cx.set_http_client(http_client);

        let ep_store = EditPredictionStore::global(cx);

        (
            ep_store,
            RequestChannels {
                predict: predict_request_rx,
            },
        )
    })
}

fn init_test(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(AppDatabase::test_new());
        let settings_store = SettingsStore::test(cx);
        cx.set_global(settings_store);
    });
}

async fn apply_edit_prediction(
    buffer_content: &str,
    completion_response: &str,
    cx: &mut TestAppContext,
) -> String {
    let fs = project::FakeFs::new(cx.executor());
    let project = Project::test(fs.clone(), [path!("/project").as_ref()], cx).await;
    let buffer = cx.new(|cx| Buffer::local(buffer_content, cx));
    let (ep_store, response) = make_test_ep_store(cx).await;
    *response.lock() = completion_response.to_string();
    let edit_prediction = run_edit_prediction(&buffer, &project, &ep_store, Point::new(1, 0), cx)
        .await
        .expect("expected an edit prediction")
        .prediction;
    buffer.update(cx, |buffer, cx| {
        buffer.edit(edit_prediction.edits.iter().cloned(), None, cx)
    });
    buffer.read_with(cx, |buffer, _| buffer.text())
}

async fn run_edit_prediction(
    buffer: &Entity<Buffer>,
    project: &Entity<Project>,
    ep_store: &Entity<EditPredictionStore>,
    cursor: Point,
    cx: &mut TestAppContext,
) -> Option<EditPredictionResult> {
    let cursor = buffer.read_with(cx, |buffer, _| buffer.anchor_before(cursor));
    ep_store.update(cx, |ep_store, cx| {
        ep_store.register_buffer(buffer, &project, cx)
    });
    cx.background_executor.run_until_parked();
    let prediction_task = ep_store.update(cx, |ep_store, cx| {
        ep_store.request_prediction(
            &project,
            buffer,
            cursor,
            PredictEditsRequestTrigger::Other,
            cx,
        )
    });
    prediction_task.await.unwrap()
}

async fn make_test_ep_store(
    cx: &mut TestAppContext,
) -> (Entity<EditPredictionStore>, Arc<Mutex<String>>) {
    let default_response = "hello world\n".to_string();
    let completion_response: Arc<Mutex<String>> = Arc::new(Mutex::new(default_response));
    cx.update(configure_fake_zeta_provider);
    let http_client = FakeHttpClient::create({
        let completion_response = completion_response.clone();
        let next_request_id = std::sync::atomic::AtomicUsize::new(1);
        move |_request| {
            let request_number = next_request_id.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let completion = FakeCompletion {
                request_id: format!("request-{request_number}"),
                text: completion_response.lock().clone(),
            };
            async move { Ok(completion_http_response(completion)) }
        }
    });
    cx.update(|cx| cx.set_http_client(http_client));

    let ep_store = cx.new(|cx| {
        let mut ep_store = EditPredictionStore::new(cx);
        ep_store.set_edit_prediction_model(EditPredictionModel::Zeta);
        ep_store
    });

    (ep_store, completion_response)
}

async fn make_sweep_prompt_test_ep_store(
    cx: &mut TestAppContext,
) -> (
    Entity<EditPredictionStore>,
    Arc<Mutex<String>>,
    Arc<Mutex<Vec<RawCompletionRequest>>>,
) {
    cx.update(|cx| {
        cx.update_global::<SettingsStore, _>(|store: &mut SettingsStore, cx| {
            store.update_user_settings(cx, |settings| {
                settings.project.all_languages.edit_predictions =
                    Some(settings::EditPredictionSettingsContent {
                        provider: Some(settings::EditPredictionProvider::OpenAiCompatibleApi),
                        open_ai_compatible_api: Some(
                            settings::CustomEditPredictionProviderSettingsContent {
                                api_url: Some("http://localhost:8080/v1/completions".to_string()),
                                model: Some("sweep-next-edit-1.5b".to_string()),
                                prompt_format: Some(
                                    settings::EditPredictionPromptFormatContent::Sweep,
                                ),
                                max_output_tokens: Some(64),
                                prediction_debounce: None,
                            },
                        ),
                        ..Default::default()
                    });
            });
        });
    });

    let default_response = String::new();
    let completion_response = Arc::new(Mutex::new(default_response));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let http_client = FakeHttpClient::create({
        let completion_response = completion_response.clone();
        let requests = requests.clone();
        let mut next_request_id = 0;
        move |request| {
            let completion_response = completion_response.clone();
            let requests = requests.clone();
            let method = request.method().clone();
            let uri = request.uri().path().to_string();
            let mut body = request.into_body();
            async move {
                match (method, uri.as_str()) {
                    (Method::POST, "/v1/completions") => {
                        let mut body_bytes = Vec::new();
                        body.read_to_end(&mut body_bytes)
                            .await
                            .expect("fake completion server should read request body");
                        let request: RawCompletionRequest =
                            serde_json::from_slice(&body_bytes).unwrap();
                        requests.lock().push(request);

                        next_request_id += 1;
                        let response = RawCompletionResponse {
                            id: format!("request-{next_request_id}"),
                            object: "text_completion".to_string(),
                            created: 0,
                            model: "sweep-next-edit-1.5b".to_string(),
                            choices: vec![RawCompletionChoice {
                                text: completion_response.lock().clone(),
                                finish_reason: Some("stop".to_string()),
                            }],
                            usage: RawCompletionUsage {
                                prompt_tokens: 0,
                                completion_tokens: 0,
                                total_tokens: 0,
                            },
                        };

                        Ok(http_client::Response::builder()
                            .status(200)
                            .body(serde_json::to_string(&response).unwrap().into())
                            .unwrap())
                    }
                    _ => Ok(http_client::Response::builder()
                        .status(404)
                        .body("Not Found".to_string().into())
                        .unwrap()),
                }
            }
        }
    });

    cx.update(|cx| {
        cx.set_http_client(http_client.clone());
    });

    let ep_store = cx.new(|cx| {
        let mut ep_store = EditPredictionStore::new(cx);
        ep_store.set_edit_prediction_model(EditPredictionModel::SweepPrompt);
        ep_store
    });

    (ep_store, completion_response, requests)
}

fn to_completion_edits(
    iterator: impl IntoIterator<Item = (Range<usize>, Arc<str>)>,
    buffer: &Entity<Buffer>,
    cx: &App,
) -> Vec<(Range<Anchor>, Arc<str>)> {
    let buffer = buffer.read(cx);
    iterator
        .into_iter()
        .map(|(range, text)| {
            (
                buffer.anchor_after(range.start)..buffer.anchor_before(range.end),
                text,
            )
        })
        .collect()
}

fn from_completion_edits(
    editor_edits: &[(Range<Anchor>, Arc<str>)],
    buffer: &Entity<Buffer>,
    cx: &App,
) -> Vec<(Range<usize>, Arc<str>)> {
    let buffer = buffer.read(cx);
    editor_edits
        .iter()
        .map(|(range, text)| {
            (
                range.start.to_offset(buffer)..range.end.to_offset(buffer),
                text.clone(),
            )
        })
        .collect()
}

#[gpui::test]
async fn test_sweep_prompt_request_prediction_diffs_rewritten_window_into_anchored_edits(
    cx: &mut TestAppContext,
) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        "/project",
        serde_json::json!({
            "main.rs": "line 0\nline 1\nline 2\n"
        }),
    )
    .await;

    let project = Project::test(fs.clone(), [path!("/project").as_ref()], cx).await;
    let buffer = project
        .update(cx, |project, cx| {
            let path = project
                .find_project_path(path!("/project/main.rs"), cx)
                .unwrap();
            project.open_buffer(path, cx)
        })
        .await
        .unwrap();

    let (ep_store, completion_response, requests) = make_sweep_prompt_test_ep_store(cx).await;
    *completion_response.lock() = "line 0\nline 1 updated\nline 2\n".to_string();

    let result = run_edit_prediction(&buffer, &project, &ep_store, Point::new(1, 0), cx)
        .await
        .expect("expected a sweep prompt prediction");
    let prediction = result.prediction;

    let edits = cx.update(|cx| from_completion_edits(&prediction.edits, &buffer, cx));
    assert_eq!(edits, vec![(13..13, " updated".into())]);

    buffer.update(cx, |buffer, cx| {
        buffer.edit(prediction.edits.iter().cloned(), None, cx)
    });
    assert_eq!(
        buffer.read_with(cx, |buffer, _| buffer.text()),
        "line 0\nline 1 updated\nline 2\n"
    );

    let requests = requests.lock();
    assert_eq!(requests.len(), 1);
    let stop_tokens = requests[0]
        .stop
        .iter()
        .map(|token| token.as_ref())
        .collect::<Vec<_>>();
    assert_eq!(stop_tokens, vec!["<|file_sep|>", "</s>"]);
}

#[gpui::test]
async fn test_sweep_prompt_request_prediction_returns_none_for_identical_rewrite(
    cx: &mut TestAppContext,
) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        "/project",
        serde_json::json!({
            "main.rs": "line 0\nline 1\nline 2\n"
        }),
    )
    .await;

    let project = Project::test(fs.clone(), [path!("/project").as_ref()], cx).await;
    let buffer = project
        .update(cx, |project, cx| {
            let path = project
                .find_project_path(path!("/project/main.rs"), cx)
                .unwrap();
            project.open_buffer(path, cx)
        })
        .await
        .unwrap();

    let (ep_store, completion_response, requests) = make_sweep_prompt_test_ep_store(cx).await;
    *completion_response.lock() = "line 0\nline 1\nline 2\n".to_string();

    let result = run_edit_prediction(&buffer, &project, &ep_store, Point::new(1, 0), cx).await;

    assert!(
        result.is_none(),
        "identical rewrites should produce no prediction"
    );

    let requests = requests.lock();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0].prompt.contains("<|file_sep|>updated/"),
        "expected Sweep-style rewrite prompt"
    );
}

#[gpui::test]
fn test_buffer_path_with_id_fallback(cx: &mut TestAppContext) {
    let buffer_1 = cx.new(|cx| Buffer::local("one", cx));
    let buffer_2 = cx.new(|cx| Buffer::local("two", cx));

    let snapshot_1 = buffer_1.read_with(cx, |buffer, _| buffer.text_snapshot());
    let snapshot_2 = buffer_2.read_with(cx, |buffer, _| buffer.text_snapshot());

    let windows_file: Arc<dyn language::File> = Arc::new(language::TestFile {
        path: util::rel_path::rel_path("src/main.rs").into(),
        root_name: "workspace".into(),
        local_root: None,
    });
    let windows_path =
        cx.read(|cx| buffer_path_with_id_fallback(Some(&windows_file), &snapshot_1, cx));
    assert_eq!(windows_path.to_string_lossy(), "workspace/src/main.rs");

    let path_1 = cx.read(|cx| buffer_path_with_id_fallback(None, &snapshot_1, cx));
    let path_2 = cx.read(|cx| buffer_path_with_id_fallback(None, &snapshot_2, cx));

    assert_eq!(
        path_1.as_ref(),
        Path::new(&format!("untitled-{}", snapshot_1.remote_id()))
    );
    assert_eq!(
        path_2.as_ref(),
        Path::new(&format!("untitled-{}", snapshot_2.remote_id()))
    );
    assert_ne!(path_1.as_ref(), path_2.as_ref());
}

// When a user had data collection enabled via the legacy KV store (with no explicit
// setting in settings.json), toggle_data_collection must read the *resolved* state
// (true) and write Some(false).
#[ctor::ctor(unsafe)]
fn init_logger() {
    zlog::init_test();
}
