use crate::TestServer;
use client::{Collaborator, LegacyUserId};
use collab::rpc::{CLEANUP_TIMEOUT, RECONNECT_TIMEOUT};

use collections::HashMap;
use futures::future;
use gpui::{BackgroundExecutor, Entity, TestAppContext};
use rpc::{RECEIVE_TIMEOUT, proto::PeerId};

#[gpui::test]
async fn test_core_channel_buffers(
    executor: BackgroundExecutor,
    cx_a: &mut TestAppContext,
    cx_b: &mut TestAppContext,
) {
    let mut server = TestServer::start(executor.clone()).await;
    let client_a = server.create_client(cx_a, "user_a").await;
    let client_b = server.create_client(cx_b, "user_b").await;

    let channel_id = server
        .make_channel("zed", None, (&client_a, cx_a), &mut [(&client_b, cx_b)])
        .await;

    // Client A joins the channel buffer
    let channel_buffer_a = client_a
        .channel_store()
        .update(cx_a, |store, cx| store.open_channel_buffer(channel_id, cx))
        .await
        .unwrap();

    // Client A edits the buffer
    let buffer_a = channel_buffer_a.read_with(cx_a, |buffer, _| buffer.buffer());
    buffer_a.update(cx_a, |buffer, cx| {
        buffer.edit([(0..0, "hello world")], None, cx)
    });
    buffer_a.update(cx_a, |buffer, cx| {
        buffer.edit([(5..5, ", cruel")], None, cx)
    });
    buffer_a.update(cx_a, |buffer, cx| {
        buffer.edit([(0..5, "goodbye")], None, cx)
    });
    buffer_a.update(cx_a, |buffer, cx| buffer.undo(cx));
    assert_eq!(buffer_text(&buffer_a, cx_a), "hello, cruel world");
    executor.run_until_parked();

    // Client B joins the channel buffer
    let channel_buffer_b = client_b
        .channel_store()
        .update(cx_b, |store, cx| store.open_channel_buffer(channel_id, cx))
        .await
        .unwrap();
    channel_buffer_b.read_with(cx_b, |buffer, _| {
        assert_collaborators(
            buffer.collaborators(),
            &[client_a.user_id(), client_b.user_id()],
        );
    });

    // Client B sees the correct text, and then edits it
    let buffer_b = channel_buffer_b.read_with(cx_b, |buffer, _| buffer.buffer());
    assert_eq!(
        buffer_b.read_with(cx_b, |buffer, _| buffer.remote_id()),
        buffer_a.read_with(cx_a, |buffer, _| buffer.remote_id())
    );
    assert_eq!(buffer_text(&buffer_b, cx_b), "hello, cruel world");
    buffer_b.update(cx_b, |buffer, cx| {
        buffer.edit([(7..12, "beautiful")], None, cx)
    });

    // Both A and B see the new edit
    executor.run_until_parked();
    assert_eq!(buffer_text(&buffer_a, cx_a), "hello, beautiful world");
    assert_eq!(buffer_text(&buffer_b, cx_b), "hello, beautiful world");

    // Client A closes the channel buffer.
    cx_a.update(|_| drop(channel_buffer_a));
    executor.run_until_parked();

    // Client B sees that client A is gone from the channel buffer.
    channel_buffer_b.read_with(cx_b, |buffer, _| {
        assert_collaborators(buffer.collaborators(), &[client_b.user_id()]);
    });

    // Client A rejoins the channel buffer
    let _channel_buffer_a = client_a
        .channel_store()
        .update(cx_a, |store, cx| store.open_channel_buffer(channel_id, cx))
        .await
        .unwrap();
    executor.run_until_parked();

    // Sanity test, make sure we saw A rejoining
    channel_buffer_b.read_with(cx_b, |buffer, _| {
        assert_collaborators(
            buffer.collaborators(),
            &[client_a.user_id(), client_b.user_id()],
        );
    });

    // Client A loses connection.
    server.forbid_connections();
    server.disconnect_client(client_a.peer_id().unwrap());
    executor.advance_clock(RECEIVE_TIMEOUT + RECONNECT_TIMEOUT);

    // Client B observes A disconnect
    channel_buffer_b.read_with(cx_b, |buffer, _| {
        assert_collaborators(buffer.collaborators(), &[client_b.user_id()]);
    });

    // TODO:
    // - Test synchronizing offline updates, what happens to A's channel buffer when A disconnects
    // - Test interaction with channel deletion while buffer is open
}

#[gpui::test]
async fn test_multiple_handles_to_channel_buffer(
    deterministic: BackgroundExecutor,
    cx_a: &mut TestAppContext,
) {
    let mut server = TestServer::start(deterministic.clone()).await;
    let client_a = server.create_client(cx_a, "user_a").await;

    let channel_id = server
        .make_channel("the-channel", None, (&client_a, cx_a), &mut [])
        .await;

    let channel_buffer_1 = client_a
        .channel_store()
        .update(cx_a, |store, cx| store.open_channel_buffer(channel_id, cx));
    let channel_buffer_2 = client_a
        .channel_store()
        .update(cx_a, |store, cx| store.open_channel_buffer(channel_id, cx));
    let channel_buffer_3 = client_a
        .channel_store()
        .update(cx_a, |store, cx| store.open_channel_buffer(channel_id, cx));

    // All concurrent tasks for opening a channel buffer return the same model handle.
    let (channel_buffer, channel_buffer_2, channel_buffer_3) =
        future::try_join3(channel_buffer_1, channel_buffer_2, channel_buffer_3)
            .await
            .unwrap();
    let channel_buffer_entity_id = channel_buffer.entity_id();
    assert_eq!(channel_buffer, channel_buffer_2);
    assert_eq!(channel_buffer, channel_buffer_3);

    channel_buffer.update(cx_a, |buffer, cx| {
        buffer.buffer().update(cx, |buffer, cx| {
            buffer.edit([(0..0, "hello")], None, cx);
        })
    });
    deterministic.run_until_parked();

    cx_a.update(|_| {
        drop(channel_buffer);
        drop(channel_buffer_2);
        drop(channel_buffer_3);
    });
    deterministic.run_until_parked();

    // The channel buffer can be reopened after dropping it.
    let channel_buffer = client_a
        .channel_store()
        .update(cx_a, |store, cx| store.open_channel_buffer(channel_id, cx))
        .await
        .unwrap();
    assert_ne!(channel_buffer.entity_id(), channel_buffer_entity_id);
    channel_buffer.update(cx_a, |buffer, cx| {
        buffer.buffer().update(cx, |buffer, _| {
            assert_eq!(buffer.text(), "hello");
        })
    });
}

#[gpui::test]
async fn test_channel_buffer_disconnect(
    deterministic: BackgroundExecutor,
    cx_a: &mut TestAppContext,
    cx_b: &mut TestAppContext,
) {
    let mut server = TestServer::start(deterministic.clone()).await;
    let client_a = server.create_client(cx_a, "user_a").await;
    let client_b = server.create_client(cx_b, "user_b").await;

    let channel_id = server
        .make_channel(
            "the-channel",
            None,
            (&client_a, cx_a),
            &mut [(&client_b, cx_b)],
        )
        .await;

    let channel_buffer_a = client_a
        .channel_store()
        .update(cx_a, |store, cx| store.open_channel_buffer(channel_id, cx))
        .await
        .unwrap();

    let channel_buffer_b = client_b
        .channel_store()
        .update(cx_b, |store, cx| store.open_channel_buffer(channel_id, cx))
        .await
        .unwrap();

    server.forbid_connections();
    server.disconnect_client(client_a.peer_id().unwrap());
    deterministic.advance_clock(RECEIVE_TIMEOUT + RECONNECT_TIMEOUT);

    channel_buffer_a.update(cx_a, |buffer, cx| {
        assert_eq!(buffer.channel(cx).unwrap().name, "the-channel");
        assert!(!buffer.is_connected());
    });

    deterministic.run_until_parked();

    server.allow_connections();
    deterministic.advance_clock(RECEIVE_TIMEOUT + RECONNECT_TIMEOUT);

    deterministic.run_until_parked();

    client_a
        .channel_store()
        .update(cx_a, |channel_store, _| {
            channel_store.remove_channel(channel_id)
        })
        .await
        .unwrap();
    deterministic.run_until_parked();

    // Channel buffer observed the deletion
    channel_buffer_b.update(cx_b, |buffer, cx| {
        assert!(buffer.channel(cx).is_none());
        assert!(!buffer.is_connected());
    });
}

#[gpui::test]
async fn test_rejoin_channel_buffer(
    deterministic: BackgroundExecutor,
    cx_a: &mut TestAppContext,
    cx_b: &mut TestAppContext,
) {
    let mut server = TestServer::start(deterministic.clone()).await;
    let client_a = server.create_client(cx_a, "user_a").await;
    let client_b = server.create_client(cx_b, "user_b").await;

    let channel_id = server
        .make_channel(
            "the-channel",
            None,
            (&client_a, cx_a),
            &mut [(&client_b, cx_b)],
        )
        .await;

    let channel_buffer_a = client_a
        .channel_store()
        .update(cx_a, |store, cx| store.open_channel_buffer(channel_id, cx))
        .await
        .unwrap();
    let channel_buffer_b = client_b
        .channel_store()
        .update(cx_b, |store, cx| store.open_channel_buffer(channel_id, cx))
        .await
        .unwrap();

    channel_buffer_a.update(cx_a, |buffer, cx| {
        buffer.buffer().update(cx, |buffer, cx| {
            buffer.edit([(0..0, "1")], None, cx);
        })
    });
    deterministic.run_until_parked();

    // Client A disconnects.
    server.forbid_connections();
    server.disconnect_client(client_a.peer_id().unwrap());

    // Both clients make an edit.
    channel_buffer_a.update(cx_a, |buffer, cx| {
        buffer.buffer().update(cx, |buffer, cx| {
            buffer.edit([(1..1, "2")], None, cx);
        })
    });
    channel_buffer_b.update(cx_b, |buffer, cx| {
        buffer.buffer().update(cx, |buffer, cx| {
            buffer.edit([(0..0, "0")], None, cx);
        })
    });

    // Both clients see their own edit.
    deterministic.run_until_parked();
    channel_buffer_a.read_with(cx_a, |buffer, cx| {
        assert_eq!(buffer.buffer().read(cx).text(), "12");
    });
    channel_buffer_b.read_with(cx_b, |buffer, cx| {
        assert_eq!(buffer.buffer().read(cx).text(), "01");
    });

    // Client A reconnects. Both clients see each other's edits, and see
    // the same collaborators.
    server.allow_connections();
    deterministic.advance_clock(RECEIVE_TIMEOUT);
    channel_buffer_a.read_with(cx_a, |buffer, cx| {
        assert_eq!(buffer.buffer().read(cx).text(), "012");
    });
    channel_buffer_b.read_with(cx_b, |buffer, cx| {
        assert_eq!(buffer.buffer().read(cx).text(), "012");
    });

    channel_buffer_a.read_with(cx_a, |buffer_a, _| {
        channel_buffer_b.read_with(cx_b, |buffer_b, _| {
            assert_eq!(buffer_a.collaborators(), buffer_b.collaborators());
        });
    });
}

#[gpui::test]
async fn test_channel_buffers_and_server_restarts(
    deterministic: BackgroundExecutor,
    cx_a: &mut TestAppContext,
    cx_b: &mut TestAppContext,
    cx_c: &mut TestAppContext,
) {
    let mut server = TestServer::start(deterministic.clone()).await;
    let client_a = server.create_client(cx_a, "user_a").await;
    let client_b = server.create_client(cx_b, "user_b").await;
    let client_c = server.create_client(cx_c, "user_c").await;

    let channel_id = server
        .make_channel(
            "the-channel",
            None,
            (&client_a, cx_a),
            &mut [(&client_b, cx_b), (&client_c, cx_c)],
        )
        .await;

    let channel_buffer_a = client_a
        .channel_store()
        .update(cx_a, |store, cx| store.open_channel_buffer(channel_id, cx))
        .await
        .unwrap();
    let channel_buffer_b = client_b
        .channel_store()
        .update(cx_b, |store, cx| store.open_channel_buffer(channel_id, cx))
        .await
        .unwrap();
    let _channel_buffer_c = client_c
        .channel_store()
        .update(cx_c, |store, cx| store.open_channel_buffer(channel_id, cx))
        .await
        .unwrap();

    channel_buffer_a.update(cx_a, |buffer, cx| {
        buffer.buffer().update(cx, |buffer, cx| {
            buffer.edit([(0..0, "1")], None, cx);
        })
    });
    deterministic.run_until_parked();

    // Client C can't reconnect.
    client_c.override_establish_connection(|_, cx| cx.spawn(async |_| future::pending().await));

    // Server stops.
    server.reset().await;
    deterministic.advance_clock(RECEIVE_TIMEOUT);

    // While the server is down, both clients make an edit.
    channel_buffer_a.update(cx_a, |buffer, cx| {
        buffer.buffer().update(cx, |buffer, cx| {
            buffer.edit([(1..1, "2")], None, cx);
        })
    });
    channel_buffer_b.update(cx_b, |buffer, cx| {
        buffer.buffer().update(cx, |buffer, cx| {
            buffer.edit([(0..0, "0")], None, cx);
        })
    });

    // Server restarts.
    server.start().await.unwrap();
    deterministic.advance_clock(CLEANUP_TIMEOUT);

    // Clients reconnects. Clients A and B see each other's edits, and see
    // that client C has disconnected.
    channel_buffer_a.read_with(cx_a, |buffer, cx| {
        assert_eq!(buffer.buffer().read(cx).text(), "012");
    });
    channel_buffer_b.read_with(cx_b, |buffer, cx| {
        assert_eq!(buffer.buffer().read(cx).text(), "012");
    });

    channel_buffer_a.read_with(cx_a, |buffer_a, _| {
        channel_buffer_b.read_with(cx_b, |buffer_b, _| {
            assert_collaborators(
                buffer_a.collaborators(),
                &[client_a.user_id(), client_b.user_id()],
            );
            assert_eq!(buffer_a.collaborators(), buffer_b.collaborators());
        });
    });
}

#[gpui::test]
async fn test_channel_buffer_operations_lost_on_reconnect(
    executor: BackgroundExecutor,
    cx_a: &mut TestAppContext,
    cx_b: &mut TestAppContext,
) {
    let mut server = TestServer::start(executor.clone()).await;
    let client_a = server.create_client(cx_a, "user_a").await;
    let client_b = server.create_client(cx_b, "user_b").await;

    let channel_id = server
        .make_channel(
            "the-channel",
            None,
            (&client_a, cx_a),
            &mut [(&client_b, cx_b)],
        )
        .await;

    // Both clients open the channel buffer.
    let channel_buffer_a = client_a
        .channel_store()
        .update(cx_a, |store, cx| store.open_channel_buffer(channel_id, cx))
        .await
        .unwrap();
    let channel_buffer_b = client_b
        .channel_store()
        .update(cx_b, |store, cx| store.open_channel_buffer(channel_id, cx))
        .await
        .unwrap();

    // Step 1: Client A makes an initial edit that syncs to B.
    channel_buffer_a.update(cx_a, |buffer, cx| {
        buffer.buffer().update(cx, |buffer, cx| {
            buffer.edit([(0..0, "a")], None, cx);
        })
    });
    executor.run_until_parked();

    // Verify both clients see "a".
    channel_buffer_a.read_with(cx_a, |buffer, cx| {
        assert_eq!(buffer.buffer().read(cx).text(), "a");
    });
    channel_buffer_b.read_with(cx_b, |buffer, cx| {
        assert_eq!(buffer.buffer().read(cx).text(), "a");
    });

    // Step 2: Disconnect client A. Do NOT advance past RECONNECT_TIMEOUT
    // so that the buffer stays in `opened_buffers` for rejoin.
    server.forbid_connections();
    server.disconnect_client(client_a.peer_id().unwrap());
    executor.run_until_parked();

    // Step 3: While disconnected, client A makes an offline edit ("b").
    // on_buffer_update fires but client.send() fails because transport is down.
    channel_buffer_a.update(cx_a, |buffer, cx| {
        buffer.buffer().update(cx, |buffer, cx| {
            buffer.edit([(1..1, "b")], None, cx);
        })
    });
    executor.run_until_parked();

    // Client A sees "ab" locally; B still sees "a".
    channel_buffer_a.read_with(cx_a, |buffer, cx| {
        assert_eq!(buffer.buffer().read(cx).text(), "ab");
    });
    channel_buffer_b.read_with(cx_b, |buffer, cx| {
        assert_eq!(buffer.buffer().read(cx).text(), "a");
    });

    // Step 4: Reconnect and make a racing edit in parallel.
    //
    // The race condition occurs when:
    // 1. Transport reconnects, handle_connect captures version V (with "b") and sends RejoinChannelBuffers
    // 2. DURING the async gap (awaiting response), user makes edit "c"
    // 3. on_buffer_update sends UpdateChannelBuffer (succeeds because transport is up)
    // 4. Server receives BOTH messages concurrently (FuturesUnordered)
    // 5. If UpdateChannelBuffer commits first, server version is inflated to include "c"
    // 6. RejoinChannelBuffers reads inflated version and sends it back
    // 7. Client's serialize_ops(inflated_version) filters out "b" (offline edit)
    //    because the inflated version's timestamp covers "b"'s timestamp

    // Get the buffer handle for spawning
    let buffer_for_edit = channel_buffer_a.read_with(cx_a, |buffer, _| buffer.buffer());

    // Spawn the edit task - it will wait for executor to run it
    let edit_task = cx_a.spawn({
        let buffer = buffer_for_edit;
        async move |mut cx| {
            let _ = buffer.update(&mut cx, |buffer, cx| {
                buffer.edit([(2..2, "c")], None, cx);
            });
        }
    });

    // Allow connections so reconnect can succeed
    server.allow_connections();

    // Advance clock to trigger reconnection attempt
    executor.advance_clock(RECEIVE_TIMEOUT);

    // Run the edit task - this races with handle_connect
    edit_task.detach();

    // Let everything settle.
    executor.run_until_parked();

    // Step 7: Read final buffer text from both clients.
    let text_a = channel_buffer_a.read_with(cx_a, |buffer, cx| buffer.buffer().read(cx).text());
    let text_b = channel_buffer_b.read_with(cx_b, |buffer, cx| buffer.buffer().read(cx).text());

    // Both clients must see the same text containing all three edits.
    assert_eq!(
        text_a, text_b,
        "Client A and B diverged! A sees {:?}, B sees {:?}. \
         Operations were lost during reconnection.",
        text_a, text_b
    );
    assert!(
        text_a.contains('a'),
        "Initial edit 'a' missing from final text {:?}",
        text_a
    );
    assert!(
        text_a.contains('b'),
        "Offline edit 'b' missing from final text {:?}. \
         This is the reconnection race bug: the offline operation was \
         filtered out by serialize_ops because the server_version was \
         inflated by a racing UpdateChannelBuffer.",
        text_a
    );
    assert!(
        text_a.contains('c'),
        "Racing edit 'c' missing from final text {:?}",
        text_a
    );

    // Step 8: Verify the invariant directly — every operation known to
    // client A must be observed by client B's version. If any operation
    // in A's history is not covered by B's version, it was lost.
    channel_buffer_a.read_with(cx_a, |buf_a, cx_a_inner| {
        let buffer_a = buf_a.buffer().read(cx_a_inner);
        let ops_a = buffer_a.operations();
        channel_buffer_b.read_with(cx_b, |buf_b, cx_b_inner| {
            let buffer_b = buf_b.buffer().read(cx_b_inner);
            let version_b = buffer_b.version();
            for (lamport, _op) in ops_a.iter() {
                assert!(
                    version_b.observed(*lamport),
                    "Operation with lamport timestamp {:?} from client A \
                     is NOT observed by client B's version. This operation \
                     was lost during reconnection.",
                    lamport
                );
            }
        });
    });
}

#[track_caller]
fn assert_collaborators(
    collaborators: &HashMap<PeerId, Collaborator>,
    ids: &[Option<LegacyUserId>],
) {
    let mut user_ids = collaborators
        .values()
        .map(|collaborator| collaborator.user_id)
        .collect::<Vec<_>>();
    user_ids.sort();
    assert_eq!(
        user_ids,
        ids.iter().map(|id| id.unwrap()).collect::<Vec<_>>()
    );
}

fn buffer_text(channel_buffer: &Entity<language::Buffer>, cx: &mut TestAppContext) -> String {
    channel_buffer.read_with(cx, |buffer, _| buffer.text())
}
