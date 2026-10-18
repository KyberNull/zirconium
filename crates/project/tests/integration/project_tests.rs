#![allow(clippy::format_collect)]

mod agent_registry_store;
mod bookmark_store;
mod color_extractor;
mod context_server_store;
mod git_store;
mod image_store;
mod manifest_tree;
mod project_search;
mod search;
mod search_history;
mod task_inventory;
mod trusted_worktrees;

use anyhow::Result;
use buffer_diff::{
    BufferDiff, DiffHunkSecondaryStatus, DiffHunkStatus, DiffHunkStatusKind, assert_hunks,
};
use collections::{HashMap, HashSet};
use encoding_rs;
use fs::{FakeFs, PathEventKind, RealFs};
use futures::StreamExt;
use git::{
    GitHostingProviderRegistry,
    repository::{RepoPath, repo_path},
    status::{DiffStat, FileStatus, StatusCode, TrackedStatus},
};
use gpui::{
    App, AppContext, BackgroundExecutor, BorrowAppContext, Entity, FutureExt, Task, TestAppContext,
    UpdateGlobal,
};
use language::{
    BufferEvent, DiskState, Language, LanguageConfig, LanguageMatcher, LineEnding, OffsetRangeExt,
    Point,
    language_settings::{LanguageSettings, LineEndingSetting},
    markdown_lang, rust_lang,
};
use parking_lot::Mutex;
use paths::tasks_file;
use postage::stream::Stream as _;
use pretty_assertions::{assert_eq, assert_matches};
use project::{
    Event, TaskContexts,
    git_store::{GitStoreEvent, Repository, RepositoryEvent, StatusEntry, pending_op},
    search::{SearchQuery, SearchResult},
    task_store::{TaskSettingsLocation, TaskStore},
    *,
};
use rand::{Rng as _, rngs::StdRng};
use serde_json::json;
#[cfg(target_os = "linux")]
use settings::{LocalSettingsKind, LocalSettingsPath};
use settings::{SettingsStore, SplicingVec};
#[cfg(not(windows))]
use std::os;
use std::{
    cell::RefCell,
    env, mem,
    num::NonZeroU32,
    ops::Range,
    path::{Path, PathBuf},
    process::Command,
    rc::Rc,
    sync::Arc,
    time::Duration,
};
use sum_tree::SumTree;
use task::{ResolvedTask, TaskContext};
use text::{Anchor, ReplicaId, ToOffset};
use unindent::Unindent as _;
use util::{
    RandomCharIter, TryFutureExt as _, maybe, path,
    paths::{PathMatcher, PathStyle},
    rel_path::{RelPath, rel_path},
    test::{TempTree, marked_text_offsets},
};
use worktree::WorktreeModelHandle as _;

#[gpui::test]
async fn test_block_via_channel(cx: &mut gpui::TestAppContext) {
    cx.executor().allow_parking();

    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    let _thread = std::thread::spawn(move || {
        #[cfg(not(target_os = "windows"))]
        std::fs::metadata("/tmp").unwrap();
        #[cfg(target_os = "windows")]
        std::fs::metadata("C:/Windows").unwrap();
        std::thread::sleep(Duration::from_millis(1000));
        tx.unbounded_send(1).unwrap();
    });
    rx.next().await.unwrap();
}

#[gpui::test]
async fn test_block_via_smol(cx: &mut gpui::TestAppContext) {
    cx.executor().allow_parking();

    let io_task = smol::unblock(move || {
        println!("sleeping on thread {:?}", std::thread::current().id());
        std::thread::sleep(Duration::from_millis(10));
        1
    });

    let task = cx.foreground_executor().spawn(async move {
        io_task.await;
    });

    task.await;
}

#[gpui::test]
async fn test_default_session_work_dirs_prefers_directory_worktrees_over_single_file_parents(
    cx: &mut gpui::TestAppContext,
) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/root"),
        json!({
            "dir-project": {
                "src": {
                    "main.rs": "fn main() {}"
                }
            },
            "single-file.rs": "fn helper() {}"
        }),
    )
    .await;

    let project = Project::test(
        fs,
        [
            Path::new(path!("/root/single-file.rs")),
            Path::new(path!("/root/dir-project")),
        ],
        cx,
    )
    .await;

    let work_dirs = project.read_with(cx, |project, cx| project.default_path_list(cx));
    let ordered_paths = work_dirs.ordered_paths().cloned().collect::<Vec<_>>();

    assert_eq!(
        ordered_paths,
        vec![
            PathBuf::from(path!("/root/dir-project")),
            PathBuf::from(path!("/root")),
        ]
    );
}

#[gpui::test]
async fn test_default_session_work_dirs_falls_back_to_home_for_empty_project(
    cx: &mut gpui::TestAppContext,
) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    let project = Project::test(fs, [], cx).await;

    let work_dirs = project.read_with(cx, |project, cx| project.default_path_list(cx));
    let ordered_paths = work_dirs.ordered_paths().cloned().collect::<Vec<_>>();

    assert_eq!(ordered_paths, vec![paths::home_dir().to_path_buf()]);
}

// NOTE:
// While POSIX symbolic links are somewhat supported on Windows, they are an opt in by the user, and thus
// we assume that they are not supported out of the box.
#[cfg(not(windows))]
#[gpui::test]
async fn test_symlinks(cx: &mut gpui::TestAppContext) {
    init_test(cx);
    cx.executor().allow_parking();

    let dir = TempTree::new(json!({
        "root": {
            "apple": "",
            "banana": {
                "carrot": {
                    "date": "",
                    "endive": "",
                }
            },
            "fennel": {
                "grape": "",
            }
        }
    }));

    let root_link_path = dir.path().join("root_link");
    os::unix::fs::symlink(dir.path().join("root"), &root_link_path).unwrap();
    os::unix::fs::symlink(
        dir.path().join("root/fennel"),
        dir.path().join("root/finnochio"),
    )
    .unwrap();

    let project = Project::test(
        Arc::new(RealFs::new(None, cx.executor())),
        [root_link_path.as_ref()],
        cx,
    )
    .await;

    project.update(cx, |project, cx| {
        let tree = project.worktrees(cx).next().unwrap().read(cx);
        assert_eq!(tree.file_count(), 5);
        assert_eq!(
            tree.entry_for_path(rel_path("fennel/grape")).unwrap().inode,
            tree.entry_for_path(rel_path("finnochio/grape"))
                .unwrap()
                .inode
        );
    });
}

#[gpui::test]
async fn test_editorconfig_support(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let dir = TempTree::new(json!({
        ".editorconfig": r#"
        root = true
        [*.rs]
            indent_style = tab
            indent_size = 3
            end_of_line = lf
            insert_final_newline = true
            trim_trailing_whitespace = true
            max_line_length = 120
        [*.js]
            tab_width = 10
            trim_trailing_whitespace = false
            max_line_length = off
        [*.json]
            trim_trailing_whitespace = true
        "#,
        ".zed": {
            "settings.json": r#"{
                "tab_size": 8,
                "hard_tabs": false,
                "ensure_final_newline_on_save": false,
                "remove_trailing_whitespace_on_save": false,
                "preferred_line_length": 64,
                "soft_wrap": "editor_width",
                "languages": {
                    "JavaScript": {
                        "remove_trailing_whitespace_on_save": true,
                    },
                    "JSON": {
                        "remove_trailing_whitespace_on_save": true,
                    },
                    "Markdown": {
                        "remove_trailing_whitespace_on_save": true,
                    },
                },
            }"#,
        },
        "a.rs": "fn a() {\n    A\n}",
        "b": {
            ".editorconfig": r#"
            [*.rs]
                indent_size = 2
                max_line_length = off,
            "#,
            "b.rs": "fn b() {\n    B\n}",
        },
        "c.js": "def c\n  C\nend",
        "d": {
            ".editorconfig": r#"
            [*.rs]
                indent_size = 1
            "#,
            "d.rs": "fn d() {\n    D\n}",
        },
        "e": {
            ".editorconfig": r#"
            [*.rs]
                indent_size = 5
                indent_style = space
                max_line_length =
            "#,
            "e.rs": "fn e() {\n    E\n}",
        },
        "README.json": "tabs are better\n",
        "README.md": "spaces are meaningful  \n",
    }));

    let path = dir.path();
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree_from_real_fs(path, path).await;
    let project = Project::test(fs, [path], cx).await;

    let language_registry = project.read_with(cx, |project, _| project.languages().clone());
    language_registry.add(js_lang());
    language_registry.add(json_lang());
    language_registry.add(markdown_lang());
    language_registry.add(rust_lang());

    let worktree = project.update(cx, |project, cx| project.worktrees(cx).next().unwrap());

    cx.executor().run_until_parked();

    let settings_for = async |path: &str, cx: &mut TestAppContext| -> LanguageSettings {
        let buffer = project
            .update(cx, |project, cx| {
                project.open_buffer((worktree.read(cx).id(), rel_path(path)), cx)
            })
            .await
            .unwrap();
        cx.update(|cx| LanguageSettings::for_buffer(&buffer.read(cx), cx).into_owned())
    };

    let settings_a = settings_for("a.rs", cx).await;
    let settings_b = settings_for("b/b.rs", cx).await;
    let settings_c = settings_for("c.js", cx).await;
    let settings_d = settings_for("d/d.rs", cx).await;
    let settings_e = settings_for("e/e.rs", cx).await;
    let settings_readme = settings_for("README.json", cx).await;
    let settings_markdown = settings_for("README.md", cx).await;
    // .editorconfig overrides .zed/settings
    assert_eq!(Some(settings_a.tab_size), NonZeroU32::new(3));
    assert_eq!(settings_a.hard_tabs, true);
    assert_eq!(settings_a.ensure_final_newline_on_save, true);
    // .editorconfig can disable trailing whitespace removal, but should not
    // re-enable it when Zed settings have disabled it.
    assert_eq!(settings_a.remove_trailing_whitespace_on_save, false);
    assert_eq!(settings_a.line_ending, LineEndingSetting::EnforceLf);
    assert_eq!(settings_a.preferred_line_length, 120);

    // .editorconfig in b/ overrides .editorconfig in root
    assert_eq!(Some(settings_b.tab_size), NonZeroU32::new(2));

    // .editorconfig in subdirectory overrides .editorconfig in root
    assert_eq!(Some(settings_d.tab_size), NonZeroU32::new(1));

    // Non-empty values in e/ are parsed and applied as usual.
    assert_eq!(Some(settings_e.tab_size), NonZeroU32::new(5));
    assert_eq!(settings_e.hard_tabs, false);
    // An empty value opts out of the inherited `max_line_length = 120`,
    // falling back to .zed/settings.json instead of rejecting the whole file.
    assert_eq!(settings_e.preferred_line_length, 64);

    // "indent_size" is not set, so "tab_width" is used
    assert_eq!(Some(settings_c.tab_size), NonZeroU32::new(10));
    assert_eq!(settings_c.remove_trailing_whitespace_on_save, false);
    assert_eq!(settings_readme.remove_trailing_whitespace_on_save, true);
    assert_eq!(settings_markdown.remove_trailing_whitespace_on_save, true);

    // When max_line_length is "off", default to .zed/settings.json
    assert_eq!(settings_b.preferred_line_length, 64);
    assert_eq!(settings_c.preferred_line_length, 64);

    // README.md should not be affected by .editorconfig's globe "*.rs"
    assert_eq!(Some(settings_readme.tab_size), NonZeroU32::new(8));
}

#[gpui::test]
async fn test_external_editorconfig_support(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/grandparent"),
        json!({
            ".editorconfig": "[*]\nindent_size = 4\n",
            "parent": {
                ".editorconfig": "[*.rs]\nindent_size = 2\n",
                "worktree": {
                    ".editorconfig": "[*.md]\nindent_size = 3\n",
                    "main.rs": "fn main() {}",
                    "README.md": "# README",
                    "other.txt": "other content",
                }
            }
        }),
    )
    .await;

    let project = Project::test(fs, [path!("/grandparent/parent/worktree").as_ref()], cx).await;

    let language_registry = project.read_with(cx, |project, _| project.languages().clone());
    language_registry.add(rust_lang());
    language_registry.add(markdown_lang());

    let worktree = project.update(cx, |project, cx| project.worktrees(cx).next().unwrap());

    cx.executor().run_until_parked();
    let settings_for = async |path: &str, cx: &mut TestAppContext| -> LanguageSettings {
        let buffer = project
            .update(cx, |project, cx| {
                project.open_buffer((worktree.read(cx).id(), rel_path(path)), cx)
            })
            .await
            .unwrap();
        cx.update(|cx| LanguageSettings::for_buffer(&buffer.read(cx), cx).into_owned())
    };

    let settings_rs = settings_for("main.rs", cx).await;
    let settings_md = settings_for("README.md", cx).await;
    let settings_txt = settings_for("other.txt", cx).await;

    // main.rs gets indent_size = 2 from parent's external .editorconfig
    assert_eq!(Some(settings_rs.tab_size), NonZeroU32::new(2));

    // README.md gets indent_size = 3 from internal worktree .editorconfig
    assert_eq!(Some(settings_md.tab_size), NonZeroU32::new(3));

    // other.txt gets indent_size = 4 from grandparent's external .editorconfig
    assert_eq!(Some(settings_txt.tab_size), NonZeroU32::new(4));
}

#[gpui::test]
async fn test_internal_editorconfig_root_stops_traversal(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/worktree"),
        json!({
            ".editorconfig": "[*]\nindent_size = 99\n",
            "src": {
                ".editorconfig": "root = true\n[*]\nindent_size = 2\n",
                "file.rs": "fn main() {}",
            }
        }),
    )
    .await;

    let project = Project::test(fs, [path!("/worktree").as_ref()], cx).await;

    let language_registry = project.read_with(cx, |project, _| project.languages().clone());
    language_registry.add(rust_lang());

    let worktree = project.update(cx, |project, cx| project.worktrees(cx).next().unwrap());

    cx.executor().run_until_parked();

    let buffer = project
        .update(cx, |project, cx| {
            project.open_buffer((worktree.read(cx).id(), rel_path("src/file.rs")), cx)
        })
        .await
        .unwrap();
    cx.update(|cx| {
        let settings = LanguageSettings::for_buffer(buffer.read(cx), cx).into_owned();
        assert_eq!(Some(settings.tab_size), NonZeroU32::new(2));
    });
}

#[gpui::test]
async fn test_external_editorconfig_root_stops_traversal(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/parent"),
        json!({
            ".editorconfig": "[*]\nindent_size = 99\n",
            "worktree": {
                ".editorconfig": "root = true\n[*]\nindent_size = 2\n",
                "file.rs": "fn main() {}",
            }
        }),
    )
    .await;

    let project = Project::test(fs, [path!("/parent/worktree").as_ref()], cx).await;

    let language_registry = project.read_with(cx, |project, _| project.languages().clone());
    language_registry.add(rust_lang());

    let worktree = project.update(cx, |project, cx| project.worktrees(cx).next().unwrap());

    cx.executor().run_until_parked();

    let buffer = project
        .update(cx, |project, cx| {
            project.open_buffer((worktree.read(cx).id(), rel_path("file.rs")), cx)
        })
        .await
        .unwrap();

    cx.update(|cx| {
        let settings = LanguageSettings::for_buffer(&buffer.read(cx), cx);

        // file.rs gets indent_size = 2 from worktree's root config, NOT 99 from parent
        assert_eq!(Some(settings.tab_size), NonZeroU32::new(2));
    });
}

#[gpui::test]
async fn test_external_editorconfig_root_in_parent_stops_traversal(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/grandparent"),
        json!({
            ".editorconfig": "[*]\nindent_size = 99\n",
            "parent": {
                ".editorconfig": "root = true\n[*]\nindent_size = 4\n",
                "worktree": {
                    "file.rs": "fn main() {}",
                }
            }
        }),
    )
    .await;

    let project = Project::test(fs, [path!("/grandparent/parent/worktree").as_ref()], cx).await;

    let language_registry = project.read_with(cx, |project, _| project.languages().clone());
    language_registry.add(rust_lang());

    let worktree = project.update(cx, |project, cx| project.worktrees(cx).next().unwrap());

    cx.executor().run_until_parked();

    let buffer = project
        .update(cx, |project, cx| {
            project.open_buffer((worktree.read(cx).id(), rel_path("file.rs")), cx)
        })
        .await
        .unwrap();

    cx.update(|cx| {
        let settings = LanguageSettings::for_buffer(&buffer.read(cx), cx);

        // file.rs gets indent_size = 4 from parent's root config, NOT 99 from grandparent
        assert_eq!(Some(settings.tab_size), NonZeroU32::new(4));
    });
}

#[gpui::test]
async fn test_external_editorconfig_shared_across_worktrees(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/parent"),
        json!({
            ".editorconfig": "root = true\n[*]\nindent_size = 5\n",
            "worktree_a": {
                "file.rs": "fn a() {}",
                ".editorconfig": "[*]\ninsert_final_newline = true\n",
            },
            "worktree_b": {
                "file.rs": "fn b() {}",
                ".editorconfig": "[*]\ninsert_final_newline = false\n",
            }
        }),
    )
    .await;

    let project = Project::test(
        fs,
        [
            path!("/parent/worktree_a").as_ref(),
            path!("/parent/worktree_b").as_ref(),
        ],
        cx,
    )
    .await;

    let language_registry = project.read_with(cx, |project, _| project.languages().clone());
    language_registry.add(rust_lang());

    cx.executor().run_until_parked();

    let worktrees: Vec<_> = cx.update(|cx| project.read(cx).worktrees(cx).collect());
    assert_eq!(worktrees.len(), 2);

    for worktree in worktrees {
        let buffer = project
            .update(cx, |project, cx| {
                project.open_buffer((worktree.read(cx).id(), rel_path("file.rs")), cx)
            })
            .await
            .unwrap();

        cx.update(|cx| {
            let settings = LanguageSettings::for_buffer(&buffer.read(cx), cx);

            // Both worktrees should get indent_size = 5 from shared parent .editorconfig
            assert_eq!(Some(settings.tab_size), NonZeroU32::new(5));
        });
    }
}

#[gpui::test]
async fn test_external_editorconfig_not_loaded_without_internal_config(
    cx: &mut gpui::TestAppContext,
) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/parent"),
        json!({
            ".editorconfig": "[*]\nindent_size = 99\n",
            "worktree": {
                "file.rs": "fn main() {}",
            }
        }),
    )
    .await;

    let project = Project::test(fs, [path!("/parent/worktree").as_ref()], cx).await;

    let language_registry = project.read_with(cx, |project, _| project.languages().clone());
    language_registry.add(rust_lang());

    let worktree = project.update(cx, |project, cx| project.worktrees(cx).next().unwrap());

    cx.executor().run_until_parked();

    let buffer = project
        .update(cx, |project, cx| {
            project.open_buffer((worktree.read(cx).id(), rel_path("file.rs")), cx)
        })
        .await
        .unwrap();

    cx.update(|cx| {
        let settings = LanguageSettings::for_buffer(&buffer.read(cx), cx);

        // file.rs should have default tab_size = 4, NOT 99 from parent's external .editorconfig
        // because without an internal .editorconfig, external configs are not loaded
        assert_eq!(Some(settings.tab_size), NonZeroU32::new(4));
    });
}

#[gpui::test]
async fn test_external_editorconfig_modification_triggers_refresh(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/parent"),
        json!({
            ".editorconfig": "[*]\nindent_size = 4\n",
            "worktree": {
                ".editorconfig": "[*]\n",
                "file.rs": "fn main() {}",
            }
        }),
    )
    .await;

    let project = Project::test(fs.clone(), [path!("/parent/worktree").as_ref()], cx).await;

    let language_registry = project.read_with(cx, |project, _| project.languages().clone());
    language_registry.add(rust_lang());

    let worktree = project.update(cx, |project, cx| project.worktrees(cx).next().unwrap());

    cx.executor().run_until_parked();

    let buffer = project
        .update(cx, |project, cx| {
            project.open_buffer((worktree.read(cx).id(), rel_path("file.rs")), cx)
        })
        .await
        .unwrap();

    cx.update(|cx| {
        let settings = LanguageSettings::for_buffer(&buffer.read(cx), cx);

        // Test initial settings: tab_size = 4 from parent's external .editorconfig
        assert_eq!(Some(settings.tab_size), NonZeroU32::new(4));
    });

    fs.atomic_write(
        PathBuf::from(path!("/parent/.editorconfig")),
        "[*]\nindent_size = 8\n".to_owned(),
    )
    .await
    .unwrap();

    cx.executor().run_until_parked();

    let buffer = project
        .update(cx, |project, cx| {
            project.open_buffer((worktree.read(cx).id(), rel_path("file.rs")), cx)
        })
        .await
        .unwrap();

    cx.update(|cx| {
        let settings = LanguageSettings::for_buffer(&buffer.read(cx), cx);

        // Test settings updated: tab_size = 8
        assert_eq!(Some(settings.tab_size), NonZeroU32::new(8));
    });
}

#[gpui::test]
async fn test_adding_worktree_discovers_external_editorconfigs(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/parent"),
        json!({
            ".editorconfig": "root = true\n[*]\nindent_size = 7\n",
            "existing_worktree": {
                ".editorconfig": "[*]\n",
                "file.rs": "fn a() {}",
            },
            "new_worktree": {
                ".editorconfig": "[*]\n",
                "file.rs": "fn b() {}",
            }
        }),
    )
    .await;

    let project = Project::test(fs, [path!("/parent/existing_worktree").as_ref()], cx).await;

    let language_registry = project.read_with(cx, |project, _| project.languages().clone());
    language_registry.add(rust_lang());

    cx.executor().run_until_parked();

    let buffer = project
        .update(cx, |project, cx| {
            let id = project.worktrees(cx).next().unwrap().read(cx).id();
            project.open_buffer((id, rel_path("file.rs")), cx)
        })
        .await
        .unwrap();

    cx.update(|cx| {
        let settings = LanguageSettings::for_buffer(&buffer.read(cx), cx).into_owned();

        // Test existing worktree has tab_size = 7
        assert_eq!(Some(settings.tab_size), NonZeroU32::new(7));
    });

    let (new_worktree, _) = project
        .update(cx, |project, cx| {
            project.find_or_create_worktree(path!("/parent/new_worktree"), true, cx)
        })
        .await
        .unwrap();

    cx.executor().run_until_parked();

    let buffer = project
        .update(cx, |project, cx| {
            project.open_buffer((new_worktree.read(cx).id(), rel_path("file.rs")), cx)
        })
        .await
        .unwrap();

    cx.update(|cx| {
        let settings = LanguageSettings::for_buffer(&buffer.read(cx), cx);

        // Verify new worktree also has tab_size = 7 from shared parent editorconfig
        assert_eq!(Some(settings.tab_size), NonZeroU32::new(7));
    });
}

#[gpui::test]
async fn test_removing_worktree_cleans_up_external_editorconfig(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/parent"),
        json!({
            ".editorconfig": "[*]\nindent_size = 6\n",
            "worktree": {
                ".editorconfig": "[*]\n",
                "file.rs": "fn main() {}",
            }
        }),
    )
    .await;

    let project = Project::test(fs, [path!("/parent/worktree").as_ref()], cx).await;

    let language_registry = project.read_with(cx, |project, _| project.languages().clone());
    language_registry.add(rust_lang());

    let worktree = project.update(cx, |project, cx| project.worktrees(cx).next().unwrap());
    let worktree_id = worktree.read_with(cx, |tree, _| tree.id());

    cx.executor().run_until_parked();

    cx.update(|cx| {
        let store = cx.global::<SettingsStore>();
        let (worktree_ids, external_paths, watcher_paths) =
            store.editorconfig_store.read(cx).test_state();

        // Test external config is loaded
        assert!(worktree_ids.contains(&worktree_id));
        assert!(!external_paths.is_empty());
        assert!(!watcher_paths.is_empty());
    });

    project.update(cx, |project, cx| {
        project.remove_worktree(worktree_id, cx);
    });

    cx.executor().run_until_parked();

    cx.update(|cx| {
        let store = cx.global::<SettingsStore>();
        let (worktree_ids, external_paths, watcher_paths) =
            store.editorconfig_store.read(cx).test_state();

        // Test worktree state, external configs, and watchers all removed
        assert!(!worktree_ids.contains(&worktree_id));
        assert!(external_paths.is_empty());
        assert!(watcher_paths.is_empty());
    });
}

#[gpui::test]
async fn test_shared_external_editorconfig_cleanup_with_multiple_worktrees(
    cx: &mut gpui::TestAppContext,
) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/parent"),
        json!({
            ".editorconfig": "root = true\n[*]\nindent_size = 5\n",
            "worktree_a": {
                ".editorconfig": "[*]\n",
                "file.rs": "fn a() {}",
            },
            "worktree_b": {
                ".editorconfig": "[*]\n",
                "file.rs": "fn b() {}",
            }
        }),
    )
    .await;

    let project = Project::test(
        fs,
        [
            path!("/parent/worktree_a").as_ref(),
            path!("/parent/worktree_b").as_ref(),
        ],
        cx,
    )
    .await;

    let language_registry = project.read_with(cx, |project, _| project.languages().clone());
    language_registry.add(rust_lang());

    cx.executor().run_until_parked();

    let (worktree_a_id, worktree_b, worktree_b_id) = cx.update(|cx| {
        let worktrees: Vec<_> = project.read(cx).worktrees(cx).collect();
        assert_eq!(worktrees.len(), 2);

        let worktree_a = &worktrees[0];
        let worktree_b = &worktrees[1];
        let worktree_a_id = worktree_a.read(cx).id();
        let worktree_b_id = worktree_b.read(cx).id();
        (worktree_a_id, worktree_b.clone(), worktree_b_id)
    });

    cx.update(|cx| {
        let store = cx.global::<SettingsStore>();
        let (worktree_ids, external_paths, _) = store.editorconfig_store.read(cx).test_state();

        // Test both worktrees have settings and share external config
        assert!(worktree_ids.contains(&worktree_a_id));
        assert!(worktree_ids.contains(&worktree_b_id));
        assert_eq!(external_paths.len(), 1); // single shared external config
    });

    project.update(cx, |project, cx| {
        project.remove_worktree(worktree_a_id, cx);
    });

    cx.executor().run_until_parked();

    cx.update(|cx| {
        let store = cx.global::<SettingsStore>();
        let (worktree_ids, external_paths, watcher_paths) =
            store.editorconfig_store.read(cx).test_state();

        // Test worktree_a is gone but external config remains for worktree_b
        assert!(!worktree_ids.contains(&worktree_a_id));
        assert!(worktree_ids.contains(&worktree_b_id));
        // External config should still exist because worktree_b uses it
        assert_eq!(external_paths.len(), 1);
        assert_eq!(watcher_paths.len(), 1);
    });

    let buffer = project
        .update(cx, |project, cx| {
            project.open_buffer((worktree_b.read(cx).id(), rel_path("file.rs")), cx)
        })
        .await
        .unwrap();

    cx.update(|cx| {
        let settings = LanguageSettings::for_buffer(&buffer.read(cx), cx);

        // Test worktree_b still has correct settings
        assert_eq!(Some(settings.tab_size), NonZeroU32::new(5));
    });
}

#[gpui::test]
async fn test_git_provider_project_setting(cx: &mut gpui::TestAppContext) {
    init_test(cx);
    cx.update(|cx| {
        GitHostingProviderRegistry::default_global(cx);
        git_hosting_providers::init(cx);
    });

    let fs = FakeFs::new(cx.executor());
    let str_path = path!("/dir");
    let path = Path::new(str_path);

    fs.insert_tree(
        path!("/dir"),
        json!({
            ".zed": {
                "settings.json": r#"{
                    "git_hosting_providers": [
                        {
                            "provider": "gitlab",
                            "base_url": "https://google.com",
                            "name": "foo"
                        }
                    ]
                }"#
            },
        }),
    )
    .await;

    let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;
    let (_worktree, _) =
        project.read_with(cx, |project, cx| project.find_worktree(path, cx).unwrap());
    cx.executor().run_until_parked();

    cx.update(|cx| {
        let provider = GitHostingProviderRegistry::global(cx);
        assert!(
            provider
                .list_hosting_providers()
                .into_iter()
                .any(|provider| provider.name() == "foo")
        );
    });

    fs.atomic_write(
        Path::new(path!("/dir/.zed/settings.json")).to_owned(),
        "{}".into(),
    )
    .await
    .unwrap();

    cx.run_until_parked();

    cx.update(|cx| {
        let provider = GitHostingProviderRegistry::global(cx);
        assert!(
            !provider
                .list_hosting_providers()
                .into_iter()
                .any(|provider| provider.name() == "foo")
        );
    });
}

#[gpui::test]
async fn test_managing_project_specific_settings(cx: &mut gpui::TestAppContext) {
    init_test(cx);
    TaskStore::init(None);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/dir"),
        json!({
            ".zed": {
                "settings.json": r#"{ "tab_size": 8 }"#,
                "tasks.json": r#"[{
                    "label": "cargo check all",
                    "command": "cargo",
                    "args": ["check", "--all"]
                },]"#,
            },
            "a": {
                "a.rs": "fn a() {\n    A\n}"
            },
            "b": {
                ".zed": {
                    "settings.json": r#"{ "tab_size": 2 }"#,
                    "tasks.json": r#"[{
                        "label": "cargo check",
                        "command": "cargo",
                        "args": ["check"]
                    },]"#,
                },
                "b.rs": "fn b() {\n  B\n}"
            }
        }),
    )
    .await;

    let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;
    let worktree = project.update(cx, |project, cx| project.worktrees(cx).next().unwrap());

    cx.executor().run_until_parked();
    let worktree_id = cx.update(|cx| {
        project.update(cx, |project, cx| {
            project.worktrees(cx).next().unwrap().read(cx).id()
        })
    });

    let mut task_contexts = TaskContexts::default();
    task_contexts.active_worktree_context = Some((worktree_id, TaskContext::default()));
    let task_contexts = Arc::new(task_contexts);

    let topmost_local_task_source_kind = TaskSourceKind::Worktree {
        id: worktree_id,
        directory_in_worktree: rel_path(".zed").into(),
        id_base: "local worktree tasks from directory \".zed\"".into(),
    };

    let buffer_a = project
        .update(cx, |project, cx| {
            project.open_buffer((worktree.read(cx).id(), rel_path("a/a.rs")), cx)
        })
        .await
        .unwrap();
    let buffer_b = project
        .update(cx, |project, cx| {
            project.open_buffer((worktree.read(cx).id(), rel_path("b/b.rs")), cx)
        })
        .await
        .unwrap();
    cx.update(|cx| {
        let settings_a = LanguageSettings::for_buffer(&buffer_a.read(cx), cx);
        let settings_b = LanguageSettings::for_buffer(&buffer_b.read(cx), cx);

        assert_eq!(settings_a.tab_size.get(), 8);
        assert_eq!(settings_b.tab_size.get(), 2);
    });

    let all_tasks = cx
        .update(|cx| get_all_tasks(&project, task_contexts.clone(), cx))
        .await
        .into_iter()
        .map(|(source_kind, task)| {
            let resolved = task.resolved;
            (
                source_kind,
                task.resolved_label,
                resolved.args,
                resolved.env,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        all_tasks,
        vec![
            (
                TaskSourceKind::Worktree {
                    id: worktree_id,
                    directory_in_worktree: rel_path("b/.zed").into(),
                    id_base: "local worktree tasks from directory \"b/.zed\"".into()
                },
                "cargo check".to_string(),
                vec!["check".to_string()],
                HashMap::default(),
            ),
            (
                topmost_local_task_source_kind.clone(),
                "cargo check all".to_string(),
                vec!["check".to_string(), "--all".to_string()],
                HashMap::default(),
            ),
        ]
    );

    let (_, resolved_task) = cx
        .update(|cx| get_all_tasks(&project, task_contexts.clone(), cx))
        .await
        .into_iter()
        .find(|(source_kind, _)| source_kind == &topmost_local_task_source_kind)
        .expect("should have one global task");
    project.update(cx, |project, cx| {
        let task_inventory = project
            .task_store()
            .read(cx)
            .task_inventory()
            .cloned()
            .unwrap();
        task_inventory.update(cx, |inventory, _| {
            inventory.task_scheduled(topmost_local_task_source_kind.clone(), resolved_task);
            inventory
                .update_file_based_tasks(
                    TaskSettingsLocation::Global(tasks_file()),
                    Some(
                        &json!([{
                            "label": "cargo check unstable",
                            "command": "cargo",
                            "args": [
                                "check",
                                "--all",
                                "--all-targets"
                            ],
                            "env": {
                                "RUSTFLAGS": "-Zunstable-options"
                            }
                        }])
                        .to_string(),
                    ),
                )
                .unwrap();
        });
    });
    cx.run_until_parked();

    let all_tasks = cx
        .update(|cx| get_all_tasks(&project, task_contexts.clone(), cx))
        .await
        .into_iter()
        .map(|(source_kind, task)| {
            let resolved = task.resolved;
            (
                source_kind,
                task.resolved_label,
                resolved.args,
                resolved.env,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        all_tasks,
        vec![
            (
                topmost_local_task_source_kind.clone(),
                "cargo check all".to_string(),
                vec!["check".to_string(), "--all".to_string()],
                HashMap::default(),
            ),
            (
                TaskSourceKind::Worktree {
                    id: worktree_id,
                    directory_in_worktree: rel_path("b/.zed").into(),
                    id_base: "local worktree tasks from directory \"b/.zed\"".into()
                },
                "cargo check".to_string(),
                vec!["check".to_string()],
                HashMap::default(),
            ),
            (
                TaskSourceKind::AbsPath {
                    abs_path: paths::tasks_file().clone(),
                    id_base: "global tasks.json".into(),
                },
                "cargo check unstable".to_string(),
                vec![
                    "check".to_string(),
                    "--all".to_string(),
                    "--all-targets".to_string(),
                ],
                HashMap::from_iter(Some((
                    "RUSTFLAGS".to_string(),
                    "-Zunstable-options".to_string()
                ))),
            ),
        ]
    );
}

#[gpui::test]
async fn test_invalid_local_tasks_shows_toast_with_doc_link(cx: &mut gpui::TestAppContext) {
    init_test(cx);
    TaskStore::init(None);

    // We need to start with a valid `.zed/tasks.json` file as otherwise the
    // event is emitted before we havd a chance to setup the event subscription.
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/dir"),
        json!({
            ".zed": {
                "tasks.json": r#"[{ "label": "valid task", "command": "echo" }]"#,
            },
            "file.rs": ""
        }),
    )
    .await;

    let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;
    let saw_toast = Rc::new(RefCell::new(false));

    // Update the `.zed/tasks.json` file with an invalid variable, so we can
    // later assert that the `Event::Toast` even is emitted.
    fs.save(
        path!("/dir/.zed/tasks.json").as_ref(),
        &r#"[{ "label": "test $ZED_FOO", "command": "echo" }]"#.into(),
        Default::default(),
    )
    .await
    .unwrap();

    project.update(cx, |_, cx| {
        let saw_toast = saw_toast.clone();

        cx.subscribe(&project, move |_, _, event: &Event, _| match event {
            Event::Toast {
                notification_id,
                message,
                link: Some(ToastLink { url, .. }),
            } => {
                assert!(notification_id.starts_with("local-tasks-"));
                assert!(message.contains("ZED_FOO"));
                assert_eq!(*url, "https://zed.dev/docs/tasks");
                *saw_toast.borrow_mut() = true;
            }
            _ => {}
        })
        .detach();
    });

    cx.run_until_parked();
    assert!(
        *saw_toast.borrow(),
        "Expected `Event::Toast` was never emitted"
    );
}

#[gpui::test]
async fn test_fallback_to_single_worktree_tasks(cx: &mut gpui::TestAppContext) {
    init_test(cx);
    TaskStore::init(None);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/dir"),
        json!({
            ".zed": {
                "tasks.json": r#"[{
                    "label": "test worktree root",
                    "command": "echo $ZED_WORKTREE_ROOT"
                }]"#,
            },
            "a": {
                "a.rs": "fn a() {\n    A\n}"
            },
        }),
    )
    .await;

    let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;
    let _worktree = project.update(cx, |project, cx| project.worktrees(cx).next().unwrap());

    cx.executor().run_until_parked();
    let worktree_id = cx.update(|cx| {
        project.update(cx, |project, cx| {
            project.worktrees(cx).next().unwrap().read(cx).id()
        })
    });

    let active_non_worktree_item_tasks = cx
        .update(|cx| {
            get_all_tasks(
                &project,
                Arc::new(TaskContexts {
                    active_item_context: Some((Some(worktree_id), None, TaskContext::default())),
                    active_worktree_context: None,
                    other_worktree_contexts: Vec::new(),
                    latest_selection: None,
                }),
                cx,
            )
        })
        .await;
    assert!(
        active_non_worktree_item_tasks.is_empty(),
        "A task can not be resolved with context with no ZED_WORKTREE_ROOT data"
    );

    let active_worktree_tasks = cx
        .update(|cx| {
            get_all_tasks(
                &project,
                Arc::new(TaskContexts {
                    active_item_context: Some((Some(worktree_id), None, TaskContext::default())),
                    active_worktree_context: Some((worktree_id, {
                        let mut worktree_context = TaskContext::default();
                        worktree_context
                            .task_variables
                            .insert(task::VariableName::WorktreeRoot, "/dir".to_string());
                        worktree_context
                    })),
                    other_worktree_contexts: Vec::new(),
                    latest_selection: None,
                }),
                cx,
            )
        })
        .await;
    assert_eq!(
        active_worktree_tasks
            .into_iter()
            .map(|(source_kind, task)| {
                let resolved = task.resolved;
                (source_kind, resolved.command.unwrap())
            })
            .collect::<Vec<_>>(),
        vec![(
            TaskSourceKind::Worktree {
                id: worktree_id,
                directory_in_worktree: rel_path(".zed").into(),
                id_base: "local worktree tasks from directory \".zed\"".into(),
            },
            "echo /dir".to_string(),
        )]
    );
}

#[gpui::test]
async fn test_rename_file_to_new_directory(cx: &mut gpui::TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.background_executor.clone());
    let expected_contents = "content";
    fs.as_fake()
        .insert_tree(
            "/root",
            json!({
                "test.txt": expected_contents
            }),
        )
        .await;

    let project = Project::test(fs, [path!("/root").as_ref()], cx).await;

    let (worktree, entry_id) = project.read_with(cx, |project, cx| {
        let worktree = project.worktrees(cx).next().unwrap();
        let entry_id = worktree
            .read(cx)
            .entry_for_path(rel_path("test.txt"))
            .unwrap()
            .id;
        (worktree, entry_id)
    });
    let worktree_id = worktree.read_with(cx, |worktree, _| worktree.id());
    let _result = project
        .update(cx, |project, cx| {
            project.rename_entry(
                entry_id,
                (worktree_id, rel_path("dir1/dir2/dir3/test.txt")).into(),
                cx,
            )
        })
        .await
        .unwrap();
    worktree.read_with(cx, |worktree, _| {
        assert!(
            worktree.entry_for_path(rel_path("test.txt")).is_none(),
            "Old file should have been removed"
        );
        assert!(
            worktree
                .entry_for_path(rel_path("dir1/dir2/dir3/test.txt"))
                .is_some(),
            "Whole directory hierarchy and the new file should have been created"
        );
    });
    assert_eq!(
        worktree
            .update(cx, |worktree, cx| {
                worktree.load_file(rel_path("dir1/dir2/dir3/test.txt"), cx)
            })
            .await
            .unwrap()
            .text
            .to_string(),
        expected_contents,
        "Moved file's contents should be preserved"
    );

    let entry_id = worktree.read_with(cx, |worktree, _| {
        worktree
            .entry_for_path(rel_path("dir1/dir2/dir3/test.txt"))
            .unwrap()
            .id
    });

    let _result = project
        .update(cx, |project, cx| {
            project.rename_entry(
                entry_id,
                (worktree_id, rel_path("dir1/dir2/test.txt")).into(),
                cx,
            )
        })
        .await
        .unwrap();
    worktree.read_with(cx, |worktree, _| {
        assert!(
            worktree.entry_for_path(rel_path("test.txt")).is_none(),
            "First file should not reappear"
        );
        assert!(
            worktree
                .entry_for_path(rel_path("dir1/dir2/dir3/test.txt"))
                .is_none(),
            "Old file should have been removed"
        );
        assert!(
            worktree
                .entry_for_path(rel_path("dir1/dir2/test.txt"))
                .is_some(),
            "No error should have occurred after moving into existing directory"
        );
    });
    assert_eq!(
        worktree
            .update(cx, |worktree, cx| {
                worktree.load_file(rel_path("dir1/dir2/test.txt"), cx)
            })
            .await
            .unwrap()
            .text
            .to_string(),
        expected_contents,
        "Moved file's contents should be preserved"
    );
}

#[gpui::test(iterations = 10)]
async fn test_save_file(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/dir"),
        json!({
            "file1": "the old contents",
        }),
    )
    .await;

    let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;
    let buffer = project
        .update(cx, |p, cx| p.open_local_buffer(path!("/dir/file1"), cx))
        .await
        .unwrap();
    buffer.update(cx, |buffer, cx| {
        assert_eq!(buffer.text(), "the old contents");
        buffer.edit([(0..0, "a line of text.\n".repeat(10 * 1024))], None, cx);
    });

    project
        .update(cx, |project, cx| project.save_buffer(buffer.clone(), cx))
        .await
        .unwrap();

    let new_text = fs
        .load(Path::new(path!("/dir/file1")))
        .await
        .unwrap()
        .replace("\r\n", "\n");
    assert_eq!(new_text, buffer.update(cx, |buffer, _| buffer.text()));
}

#[gpui::test(iterations = 30)]
async fn test_file_changes_multiple_times_on_disk(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/dir"),
        json!({
            "file1": "the original contents",
        }),
    )
    .await;

    let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;
    let worktree = project.read_with(cx, |project, cx| project.worktrees(cx).next().unwrap());
    let buffer = project
        .update(cx, |p, cx| p.open_local_buffer(path!("/dir/file1"), cx))
        .await
        .unwrap();

    // Change the buffer's file on disk, and then wait for the file change
    // to be detected by the worktree, so that the buffer starts reloading.
    fs.save(
        path!("/dir/file1").as_ref(),
        &"the first contents".into(),
        Default::default(),
    )
    .await
    .unwrap();
    worktree.next_event(cx).await;

    // Change the buffer's file again. Depending on the random seed, the
    // previous file change may still be in progress.
    fs.save(
        path!("/dir/file1").as_ref(),
        &"the second contents".into(),
        Default::default(),
    )
    .await
    .unwrap();
    worktree.next_event(cx).await;

    cx.executor().run_until_parked();
    let on_disk_text = fs.load(Path::new(path!("/dir/file1"))).await.unwrap();
    buffer.read_with(cx, |buffer, _| {
        assert_eq!(buffer.text(), on_disk_text);
        assert!(!buffer.is_dirty(), "buffer should not be dirty");
        assert!(!buffer.has_conflict(), "buffer should not be dirty");
    });
}

#[gpui::test(iterations = 30)]
async fn test_edit_buffer_while_it_reloads(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/dir"),
        json!({
            "file1": "the original contents",
        }),
    )
    .await;

    let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;
    let worktree = project.read_with(cx, |project, cx| project.worktrees(cx).next().unwrap());
    let buffer = project
        .update(cx, |p, cx| p.open_local_buffer(path!("/dir/file1"), cx))
        .await
        .unwrap();

    // Change the buffer's file on disk, and then wait for the file change
    // to be detected by the worktree, so that the buffer starts reloading.
    fs.save(
        path!("/dir/file1").as_ref(),
        &"the first contents".into(),
        Default::default(),
    )
    .await
    .unwrap();
    worktree.next_event(cx).await;

    cx.executor()
        .spawn(cx.executor().simulate_random_delay())
        .await;

    // Perform a noop edit, causing the buffer's version to increase.
    buffer.update(cx, |buffer, cx| {
        buffer.edit([(0..0, " ")], None, cx);
        buffer.undo(cx);
    });

    cx.executor().run_until_parked();
    let on_disk_text = fs.load(Path::new(path!("/dir/file1"))).await.unwrap();
    buffer.read_with(cx, |buffer, _| {
        let buffer_text = buffer.text();
        if buffer_text == on_disk_text {
            assert!(
                !buffer.is_dirty() && !buffer.has_conflict(),
                "buffer shouldn't be dirty. text: {buffer_text:?}, disk text: {on_disk_text:?}",
            );
        }
        // If the file change occurred while the buffer was processing the first
        // change, the buffer will be in a conflicting state.
        else {
            assert!(buffer.is_dirty(), "buffer should report that it is dirty. text: {buffer_text:?}, disk text: {on_disk_text:?}");
            assert!(buffer.has_conflict(), "buffer should report that it is dirty. text: {buffer_text:?}, disk text: {on_disk_text:?}");
        }
    });
}

#[gpui::test]
async fn test_save_in_single_file_worktree(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/dir"),
        json!({
            "file1": "the old contents",
        }),
    )
    .await;

    let project = Project::test(fs.clone(), [path!("/dir/file1").as_ref()], cx).await;
    let buffer = project
        .update(cx, |p, cx| p.open_local_buffer(path!("/dir/file1"), cx))
        .await
        .unwrap();
    buffer.update(cx, |buffer, cx| {
        buffer.edit([(0..0, "a line of text.\n".repeat(10 * 1024))], None, cx);
    });

    project
        .update(cx, |project, cx| project.save_buffer(buffer.clone(), cx))
        .await
        .unwrap();

    let new_text = fs
        .load(Path::new(path!("/dir/file1")))
        .await
        .unwrap()
        .replace("\r\n", "\n");
    assert_eq!(new_text, buffer.update(cx, |buffer, _| buffer.text()));
}

#[gpui::test]
async fn test_save_as(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/dir", json!({})).await;

    let project = Project::test(fs.clone(), ["/dir".as_ref()], cx).await;

    let languages = project.update(cx, |project, _| project.languages().clone());
    languages.add(rust_lang());

    let buffer = project.update(cx, |project, cx| {
        project.create_local_buffer("", None, false, cx)
    });
    buffer.update(cx, |buffer, cx| {
        buffer.edit([(0..0, "abc")], None, cx);
        assert!(buffer.is_dirty());
        assert!(!buffer.has_conflict());
        assert_eq!(buffer.language().unwrap().name(), "Plain Text");
    });
    project
        .update(cx, |project, cx| {
            let worktree_id = project.worktrees(cx).next().unwrap().read(cx).id();
            let path = ProjectPath {
                worktree_id,
                path: rel_path("file1.rs").into(),
            };
            project.save_buffer_as(buffer.clone(), path, cx)
        })
        .await
        .unwrap();
    assert_eq!(fs.load(Path::new("/dir/file1.rs")).await.unwrap(), "abc");

    cx.executor().run_until_parked();
    buffer.update(cx, |buffer, cx| {
        assert_eq!(
            buffer.file().unwrap().full_path(cx),
            Path::new("dir/file1.rs")
        );
        assert!(!buffer.is_dirty());
        assert!(!buffer.has_conflict());
        assert_eq!(buffer.language().unwrap().name(), "Rust");
    });

    let opened_buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer("/dir/file1.rs", cx)
        })
        .await
        .unwrap();
    assert_eq!(opened_buffer, buffer);
}

#[gpui::test]
async fn test_save_as_existing_file(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;

    fs.insert_tree(
        path!("/dir"),
        json!({
                "data_a.txt": "data about a"
        }),
    )
    .await;

    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer(path!("/dir/data_a.txt"), cx)
        })
        .await
        .unwrap();

    buffer.update(cx, |buffer, cx| {
        buffer.edit([(11..12, "b")], None, cx);
    });

    // Save buffer's contents as a new file and confirm that the buffer's now
    // associated with `data_b.txt` instead of `data_a.txt`, confirming that the
    // file associated with the buffer has now been updated to `data_b.txt`
    project
        .update(cx, |project, cx| {
            let worktree_id = project.worktrees(cx).next().unwrap().read(cx).id();
            let new_path = ProjectPath {
                worktree_id,
                path: rel_path("data_b.txt").into(),
            };

            project.save_buffer_as(buffer.clone(), new_path, cx)
        })
        .await
        .unwrap();

    buffer.update(cx, |buffer, cx| {
        assert_eq!(
            buffer.file().unwrap().full_path(cx),
            Path::new("dir/data_b.txt")
        )
    });

    // Open the original `data_a.txt` file, confirming that its contents are
    // unchanged and the resulting buffer's associated file is `data_a.txt`.
    let original_buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer(path!("/dir/data_a.txt"), cx)
        })
        .await
        .unwrap();

    original_buffer.update(cx, |buffer, cx| {
        assert_eq!(buffer.text(), "data about a");
        assert_eq!(
            buffer.file().unwrap().full_path(cx),
            Path::new("dir/data_a.txt")
        )
    });
}

#[gpui::test(retries = 5)]
async fn test_rescan_and_remote_updates(cx: &mut gpui::TestAppContext) {
    use worktree::WorktreeModelHandle as _;

    init_test(cx);
    cx.executor().allow_parking();

    let dir = TempTree::new(json!({
        "a": {
            "file1": "",
            "file2": "",
            "file3": "",
        },
        "b": {
            "c": {
                "file4": "",
                "file5": "",
            }
        }
    }));

    let project = Project::test(Arc::new(RealFs::new(None, cx.executor())), [dir.path()], cx).await;

    let buffer_for_path = |path: &'static str, cx: &mut gpui::TestAppContext| {
        let buffer = project.update(cx, |p, cx| p.open_local_buffer(dir.path().join(path), cx));
        async move { buffer.await.unwrap() }
    };
    let id_for_path = |path: &'static str, cx: &mut gpui::TestAppContext| {
        project.update(cx, |project, cx| {
            let tree = project.worktrees(cx).next().unwrap();
            tree.read(cx)
                .entry_for_path(rel_path(path))
                .unwrap_or_else(|| panic!("no entry for path {}", path))
                .id
        })
    };

    let buffer2 = buffer_for_path("a/file2", cx).await;
    let buffer3 = buffer_for_path("a/file3", cx).await;
    let buffer4 = buffer_for_path("b/c/file4", cx).await;
    let buffer5 = buffer_for_path("b/c/file5", cx).await;

    let file2_id = id_for_path("a/file2", cx);
    let file3_id = id_for_path("a/file3", cx);
    let file4_id = id_for_path("b/c/file4", cx);

    // Create a remote copy of this worktree.
    let tree = project.update(cx, |project, cx| project.worktrees(cx).next().unwrap());
    let metadata = tree.update(cx, |tree, _| tree.metadata_proto());

    let updates = Arc::new(Mutex::new(Vec::new()));
    tree.update(cx, |tree, cx| {
        let updates = updates.clone();
        tree.observe_updates(0, cx, move |update| {
            updates.lock().push(update);
            async { true }
        });
    });

    let remote = cx.update(|cx| {
        Worktree::remote(
            0,
            ReplicaId::REMOTE_SERVER,
            metadata,
            project.read(cx).client().into(),
            project.read(cx).path_style(cx),
            cx,
        )
    });

    cx.executor().run_until_parked();

    cx.update(|cx| {
        assert!(!buffer2.read(cx).is_dirty());
        assert!(!buffer3.read(cx).is_dirty());
        assert!(!buffer4.read(cx).is_dirty());
        assert!(!buffer5.read(cx).is_dirty());
    });

    // Rename and delete files and directories.
    tree.flush_fs_events(cx).await;
    std::fs::rename(dir.path().join("a/file3"), dir.path().join("b/c/file3")).unwrap();
    std::fs::remove_file(dir.path().join("b/c/file5")).unwrap();
    std::fs::rename(dir.path().join("b/c"), dir.path().join("d")).unwrap();
    std::fs::rename(dir.path().join("a/file2"), dir.path().join("a/file2.new")).unwrap();
    tree.flush_fs_events(cx).await;

    cx.update(|app| {
        assert_eq!(
            tree.read(app).paths().collect::<Vec<_>>(),
            vec![
                rel_path("a"),
                rel_path("a/file1"),
                rel_path("a/file2.new"),
                rel_path("b"),
                rel_path("d"),
                rel_path("d/file3"),
                rel_path("d/file4"),
            ]
        );
    });

    assert_eq!(id_for_path("a/file2.new", cx), file2_id);
    assert_eq!(id_for_path("d/file3", cx), file3_id);
    assert_eq!(id_for_path("d/file4", cx), file4_id);

    cx.update(|cx| {
        assert_eq!(
            buffer2.read(cx).file().unwrap().path().as_ref(),
            rel_path("a/file2.new")
        );
        assert_eq!(
            buffer3.read(cx).file().unwrap().path().as_ref(),
            rel_path("d/file3")
        );
        assert_eq!(
            buffer4.read(cx).file().unwrap().path().as_ref(),
            rel_path("d/file4")
        );
        assert_eq!(
            buffer5.read(cx).file().unwrap().path().as_ref(),
            rel_path("b/c/file5")
        );

        assert_matches!(
            buffer2.read(cx).file().unwrap().disk_state(),
            DiskState::Present { .. }
        );
        assert_matches!(
            buffer3.read(cx).file().unwrap().disk_state(),
            DiskState::Present { .. }
        );
        assert_matches!(
            buffer4.read(cx).file().unwrap().disk_state(),
            DiskState::Present { .. }
        );
        assert_eq!(
            buffer5.read(cx).file().unwrap().disk_state(),
            DiskState::Deleted
        );
    });

    // Update the remote worktree. Check that it becomes consistent with the
    // local worktree.
    cx.executor().run_until_parked();

    remote.update(cx, |remote, _| {
        for update in updates.lock().drain(..) {
            remote.as_remote_mut().unwrap().update_from_remote(update);
        }
    });
    cx.executor().run_until_parked();
    remote.update(cx, |remote, _| {
        assert_eq!(
            remote.paths().collect::<Vec<_>>(),
            vec![
                rel_path("a"),
                rel_path("a/file1"),
                rel_path("a/file2.new"),
                rel_path("b"),
                rel_path("d"),
                rel_path("d/file3"),
                rel_path("d/file4"),
            ]
        );
    });
}

#[cfg(target_os = "linux")]
#[gpui::test(retries = 5)]
async fn test_recreated_directory_receives_child_events(cx: &mut gpui::TestAppContext) {
    init_test(cx);
    cx.executor().allow_parking();

    let dir = TempTree::new(json!({}));
    let project = Project::test(Arc::new(RealFs::new(None, cx.executor())), [dir.path()], cx).await;
    let tree = project.update(cx, |project, cx| project.worktrees(cx).next().unwrap());

    tree.flush_fs_events(cx).await;

    let repro_dir = dir.path().join("repro");
    std::fs::create_dir(&repro_dir).unwrap();
    tree.flush_fs_events(cx).await;

    cx.update(|cx| {
        assert!(tree.read(cx).entry_for_path(rel_path("repro")).is_some());
    });

    std::fs::remove_dir_all(&repro_dir).unwrap();
    tree.flush_fs_events(cx).await;

    cx.update(|cx| {
        assert!(tree.read(cx).entry_for_path(rel_path("repro")).is_none());
    });

    std::fs::create_dir(&repro_dir).unwrap();
    tree.flush_fs_events(cx).await;

    cx.update(|cx| {
        assert!(tree.read(cx).entry_for_path(rel_path("repro")).is_some());
    });

    std::fs::write(repro_dir.join("repro-marker"), "").unwrap();
    tree.flush_fs_events(cx).await;

    cx.update(|cx| {
        assert!(
            tree.read(cx)
                .entry_for_path(rel_path("repro/repro-marker"))
                .is_some()
        );
    });
}

#[gpui::test(iterations = 10)]
async fn test_buffer_identity_across_renames(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/dir"),
        json!({
            "a": {
                "file1": "",
            }
        }),
    )
    .await;

    let project = Project::test(fs, [Path::new(path!("/dir"))], cx).await;
    let tree = project.update(cx, |project, cx| project.worktrees(cx).next().unwrap());
    let tree_id = tree.update(cx, |tree, _| tree.id());

    let id_for_path = |path: &'static str, cx: &mut gpui::TestAppContext| {
        project.update(cx, |project, cx| {
            let tree = project.worktrees(cx).next().unwrap();
            tree.read(cx)
                .entry_for_path(rel_path(path))
                .unwrap_or_else(|| panic!("no entry for path {}", path))
                .id
        })
    };

    let dir_id = id_for_path("a", cx);
    let file_id = id_for_path("a/file1", cx);
    let buffer = project
        .update(cx, |p, cx| {
            p.open_buffer((tree_id, rel_path("a/file1")), cx)
        })
        .await
        .unwrap();
    buffer.update(cx, |buffer, _| assert!(!buffer.is_dirty()));

    project
        .update(cx, |project, cx| {
            project.rename_entry(dir_id, (tree_id, rel_path("b")).into(), cx)
        })
        .unwrap()
        .await
        .into_included()
        .unwrap();
    cx.executor().run_until_parked();

    assert_eq!(id_for_path("b", cx), dir_id);
    assert_eq!(id_for_path("b/file1", cx), file_id);
    buffer.update(cx, |buffer, _| assert!(!buffer.is_dirty()));
}

#[gpui::test]
async fn test_buffer_deduping(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        "/dir",
        json!({
            "a.txt": "a-contents",
            "b.txt": "b-contents",
        }),
    )
    .await;

    let project = Project::test(fs.clone(), ["/dir".as_ref()], cx).await;

    // Spawn multiple tasks to open paths, repeating some paths.
    let (buffer_a_1, buffer_b, buffer_a_2) = project.update(cx, |p, cx| {
        (
            p.open_local_buffer("/dir/a.txt", cx),
            p.open_local_buffer("/dir/b.txt", cx),
            p.open_local_buffer("/dir/a.txt", cx),
        )
    });

    let buffer_a_1 = buffer_a_1.await.unwrap();
    let buffer_a_2 = buffer_a_2.await.unwrap();
    let buffer_b = buffer_b.await.unwrap();
    assert_eq!(buffer_a_1.update(cx, |b, _| b.text()), "a-contents");
    assert_eq!(buffer_b.update(cx, |b, _| b.text()), "b-contents");

    // There is only one buffer per path.
    let buffer_a_id = buffer_a_1.entity_id();
    assert_eq!(buffer_a_2.entity_id(), buffer_a_id);

    // Open the same path again while it is still open.
    drop(buffer_a_1);
    let buffer_a_3 = project
        .update(cx, |p, cx| p.open_local_buffer("/dir/a.txt", cx))
        .await
        .unwrap();

    // There's still only one buffer per path.
    assert_eq!(buffer_a_3.entity_id(), buffer_a_id);
}

#[gpui::test]
async fn test_buffer_is_dirty(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/dir"),
        json!({
            "file1": "abc",
            "file2": "def",
            "file3": "ghi",
        }),
    )
    .await;

    let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;

    let buffer1 = project
        .update(cx, |p, cx| p.open_local_buffer(path!("/dir/file1"), cx))
        .await
        .unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));

    // initially, the buffer isn't dirty.
    buffer1.update(cx, |buffer, cx| {
        cx.subscribe(&buffer1, {
            let events = events.clone();
            move |_, _, event, _| match event {
                BufferEvent::Operation { .. } => {}
                _ => events.lock().push(event.clone()),
            }
        })
        .detach();

        assert!(!buffer.is_dirty());
        assert!(events.lock().is_empty());

        buffer.edit([(1..2, "")], None, cx);
    });

    // after the first edit, the buffer is dirty, and emits a dirtied event.
    buffer1.update(cx, |buffer, cx| {
        assert!(buffer.text() == "ac");
        assert!(buffer.is_dirty());
        assert_eq!(
            *events.lock(),
            &[
                language::BufferEvent::Edited {
                    source: language::BufferEditSource::User
                },
                language::BufferEvent::DirtyChanged
            ]
        );
        events.lock().clear();
        buffer.did_save(
            buffer.version(),
            buffer.file().unwrap().disk_state().mtime(),
            cx,
        );
    });

    // after saving, the buffer is not dirty, and emits a saved event.
    buffer1.update(cx, |buffer, cx| {
        assert!(!buffer.is_dirty());
        assert_eq!(*events.lock(), &[language::BufferEvent::Saved]);
        events.lock().clear();

        buffer.edit([(1..1, "B")], None, cx);
        buffer.edit([(2..2, "D")], None, cx);
    });

    // after editing again, the buffer is dirty, and emits another dirty event.
    buffer1.update(cx, |buffer, cx| {
        assert!(buffer.text() == "aBDc");
        assert!(buffer.is_dirty());
        assert_eq!(
            *events.lock(),
            &[
                language::BufferEvent::Edited {
                    source: language::BufferEditSource::User
                },
                language::BufferEvent::DirtyChanged,
                language::BufferEvent::Edited {
                    source: language::BufferEditSource::User
                },
            ],
        );
        events.lock().clear();

        // After restoring the buffer to its previously-saved state,
        // the buffer is not considered dirty anymore.
        buffer.edit([(1..3, "")], None, cx);
        assert!(buffer.text() == "ac");
        assert!(!buffer.is_dirty());
    });

    assert_eq!(
        *events.lock(),
        &[
            language::BufferEvent::Edited {
                source: language::BufferEditSource::User
            },
            language::BufferEvent::DirtyChanged
        ]
    );

    // When a file is deleted, it is not considered dirty.
    let events = Arc::new(Mutex::new(Vec::new()));
    let buffer2 = project
        .update(cx, |p, cx| p.open_local_buffer(path!("/dir/file2"), cx))
        .await
        .unwrap();
    buffer2.update(cx, |_, cx| {
        cx.subscribe(&buffer2, {
            let events = events.clone();
            move |_, _, event, _| match event {
                BufferEvent::Operation { .. } => {}
                _ => events.lock().push(event.clone()),
            }
        })
        .detach();
    });

    fs.remove_file(path!("/dir/file2").as_ref(), Default::default())
        .await
        .unwrap();
    cx.executor().run_until_parked();
    buffer2.update(cx, |buffer, _| assert!(!buffer.is_dirty()));
    assert_eq!(
        mem::take(&mut *events.lock()),
        &[language::BufferEvent::FileHandleChanged]
    );

    // Buffer becomes dirty when edited.
    buffer2.update(cx, |buffer, cx| {
        buffer.edit([(2..3, "")], None, cx);
        assert_eq!(buffer.is_dirty(), true);
    });
    assert_eq!(
        mem::take(&mut *events.lock()),
        &[
            language::BufferEvent::Edited {
                source: language::BufferEditSource::User
            },
            language::BufferEvent::DirtyChanged
        ]
    );

    // Buffer becomes clean again when all of its content is removed, because
    // the file was deleted.
    buffer2.update(cx, |buffer, cx| {
        buffer.edit([(0..2, "")], None, cx);
        assert_eq!(buffer.is_empty(), true);
        assert_eq!(buffer.is_dirty(), false);
    });
    assert_eq!(
        *events.lock(),
        &[
            language::BufferEvent::Edited {
                source: language::BufferEditSource::User
            },
            language::BufferEvent::DirtyChanged
        ]
    );

    // When a file is already dirty when deleted, we don't emit a Dirtied event.
    let events = Arc::new(Mutex::new(Vec::new()));
    let buffer3 = project
        .update(cx, |p, cx| p.open_local_buffer(path!("/dir/file3"), cx))
        .await
        .unwrap();
    buffer3.update(cx, |_, cx| {
        cx.subscribe(&buffer3, {
            let events = events.clone();
            move |_, _, event, _| match event {
                BufferEvent::Operation { .. } => {}
                _ => events.lock().push(event.clone()),
            }
        })
        .detach();
    });

    buffer3.update(cx, |buffer, cx| {
        buffer.edit([(0..0, "x")], None, cx);
    });
    events.lock().clear();
    fs.remove_file(path!("/dir/file3").as_ref(), Default::default())
        .await
        .unwrap();
    cx.executor().run_until_parked();
    assert_eq!(*events.lock(), &[language::BufferEvent::FileHandleChanged]);
    cx.update(|cx| assert!(buffer3.read(cx).is_dirty()));
}

#[gpui::test]
async fn test_dirty_buffer_reloads_after_undo(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/dir"),
        json!({
            "file.txt": "version 1",
        }),
    )
    .await;

    let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;
    let buffer = project
        .update(cx, |p, cx| p.open_local_buffer(path!("/dir/file.txt"), cx))
        .await
        .unwrap();

    buffer.read_with(cx, |buffer, _| {
        assert_eq!(buffer.text(), "version 1");
        assert!(!buffer.is_dirty());
    });

    // User makes an edit, making the buffer dirty.
    buffer.update(cx, |buffer, cx| {
        buffer.edit([(0..0, "user edit: ")], None, cx);
    });

    buffer.read_with(cx, |buffer, _| {
        assert!(buffer.is_dirty());
        assert_eq!(buffer.text(), "user edit: version 1");
    });

    // External tool writes new content while buffer is dirty.
    // file_updated() updates the File but suppresses ReloadNeeded.
    fs.save(
        path!("/dir/file.txt").as_ref(),
        &"version 2 from external tool".into(),
        Default::default(),
    )
    .await
    .unwrap();
    cx.executor().run_until_parked();

    buffer.read_with(cx, |buffer, _| {
        assert!(buffer.has_conflict());
        assert_eq!(buffer.text(), "user edit: version 1");
    });

    // User undoes their edit. Buffer becomes clean, but disk has different
    // content. did_edit() detects the dirty->clean transition and checks if
    // disk changed while dirty. Since mtime differs from saved_mtime, it
    // emits ReloadNeeded.
    buffer.update(cx, |buffer, cx| {
        buffer.undo(cx);
    });
    cx.executor().run_until_parked();

    buffer.read_with(cx, |buffer, _| {
        assert_eq!(
            buffer.text(),
            "version 2 from external tool",
            "buffer should reload from disk after undo makes it clean"
        );
        assert!(!buffer.is_dirty());
    });
}

#[gpui::test]
async fn test_buffer_file_change_to_binary_fails(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/dir"),
        json!({
            "file.txt": "",
        }),
    )
    .await;

    let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;
    let buffer = project
        .update(cx, |p, cx| p.open_local_buffer(path!("/dir/file.txt"), cx))
        .await
        .unwrap();

    fs.write(
        path!("/dir/file.txt").as_ref(),
        b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01",
    )
    .await
    .unwrap();
    cx.executor().run_until_parked();

    // Test that existing buffer is left untouched
    buffer.read_with(cx, |buffer, _| {
        assert_eq!(buffer.text(), "");
    });
}

#[gpui::test]
async fn test_buffer_file_changes_on_disk(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let (initial_contents, initial_offsets) =
        marked_text_offsets("one twoˇ\nthree ˇfourˇ five\nsixˇ seven\n");
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/dir"),
        json!({
            "the-file": initial_contents,
        }),
    )
    .await;
    let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;
    let buffer = project
        .update(cx, |p, cx| p.open_local_buffer(path!("/dir/the-file"), cx))
        .await
        .unwrap();

    let anchors = initial_offsets
        .iter()
        .map(|offset| buffer.update(cx, |b, _| b.anchor_before(offset)))
        .collect::<Vec<_>>();

    // Change the file on disk, adding two new lines of text, and removing
    // one line.
    buffer.update(cx, |buffer, _| {
        assert!(!buffer.is_dirty());
        assert!(!buffer.has_conflict());
    });

    let (new_contents, new_offsets) =
        marked_text_offsets("oneˇ\nthree ˇFOURˇ five\nsixtyˇ seven\n");
    fs.save(
        path!("/dir/the-file").as_ref(),
        &new_contents.as_str().into(),
        LineEnding::Unix,
    )
    .await
    .unwrap();

    // Because the buffer was not modified, it is reloaded from disk. Its
    // contents are edited according to the diff between the old and new
    // file contents.
    cx.executor().run_until_parked();
    buffer.update(cx, |buffer, _| {
        assert_eq!(buffer.text(), new_contents);
        assert!(!buffer.is_dirty());
        assert!(!buffer.has_conflict());

        let anchor_offsets = anchors
            .iter()
            .map(|anchor| anchor.to_offset(&*buffer))
            .collect::<Vec<_>>();
        assert_eq!(anchor_offsets, new_offsets);
    });

    // Modify the buffer
    buffer.update(cx, |buffer, cx| {
        buffer.edit([(0..0, " ")], None, cx);
        assert!(buffer.is_dirty());
        assert!(!buffer.has_conflict());
    });

    // Change the file on disk again, adding blank lines to the beginning.
    fs.save(
        path!("/dir/the-file").as_ref(),
        &"\n\n\nAAAA\naaa\nBB\nbbbbb\n".into(),
        LineEnding::Unix,
    )
    .await
    .unwrap();

    // Because the buffer is modified, it doesn't reload from disk, but is
    // marked as having a conflict.
    cx.executor().run_until_parked();
    buffer.update(cx, |buffer, _| {
        assert_eq!(buffer.text(), " ".to_string() + &new_contents);
        assert!(buffer.has_conflict());
    });
}

#[gpui::test]
async fn test_buffer_line_endings(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/dir"),
        json!({
            "file1": "a\nb\nc\n",
            "file2": "one\r\ntwo\r\nthree\r\n",
        }),
    )
    .await;

    let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;
    let buffer1 = project
        .update(cx, |p, cx| p.open_local_buffer(path!("/dir/file1"), cx))
        .await
        .unwrap();
    let buffer2 = project
        .update(cx, |p, cx| p.open_local_buffer(path!("/dir/file2"), cx))
        .await
        .unwrap();

    buffer1.update(cx, |buffer, _| {
        assert_eq!(buffer.text(), "a\nb\nc\n");
        assert_eq!(buffer.line_ending(), LineEnding::Unix);
    });
    buffer2.update(cx, |buffer, _| {
        assert_eq!(buffer.text(), "one\ntwo\nthree\n");
        assert_eq!(buffer.line_ending(), LineEnding::Windows);
    });

    // Change a file's line endings on disk from unix to windows. The buffer's
    // state updates correctly.
    fs.save(
        path!("/dir/file1").as_ref(),
        &"aaa\nb\nc\n".into(),
        LineEnding::Windows,
    )
    .await
    .unwrap();
    cx.executor().run_until_parked();
    buffer1.update(cx, |buffer, _| {
        assert_eq!(buffer.text(), "aaa\nb\nc\n");
        assert_eq!(buffer.line_ending(), LineEnding::Windows);
    });

    // Save a file with windows line endings. The file is written correctly.
    buffer2.update(cx, |buffer, cx| {
        buffer.set_text("one\ntwo\nthree\nfour\n", cx);
    });
    project
        .update(cx, |project, cx| project.save_buffer(buffer2, cx))
        .await
        .unwrap();
    assert_eq!(
        fs.load(path!("/dir/file2").as_ref()).await.unwrap(),
        "one\r\ntwo\r\nthree\r\nfour\r\n",
    );
}

#[gpui::test]
async fn test_line_ending_user_settings_on_format(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let cases = [
        (
            "default",
            None,
            [
                ("crlf_file.rs", LineEnding::Windows),
                ("lf_file.rs", LineEnding::Unix),
                ("no_newline.rs", LineEnding::default()),
            ],
        ),
        (
            "detect",
            Some(LineEndingSetting::Detect),
            [
                ("crlf_file.rs", LineEnding::Windows),
                ("lf_file.rs", LineEnding::Unix),
                ("no_newline.rs", LineEnding::default()),
            ],
        ),
        (
            "prefer_lf",
            Some(LineEndingSetting::PreferLf),
            [
                ("crlf_file.rs", LineEnding::Windows),
                ("lf_file.rs", LineEnding::Unix),
                ("no_newline.rs", LineEnding::Unix),
            ],
        ),
        (
            "prefer_crlf",
            Some(LineEndingSetting::PreferCrlf),
            [
                ("crlf_file.rs", LineEnding::Windows),
                ("lf_file.rs", LineEnding::Unix),
                ("no_newline.rs", LineEnding::Windows),
            ],
        ),
        (
            "enforce_lf",
            Some(LineEndingSetting::EnforceLf),
            [
                ("crlf_file.rs", LineEnding::Unix),
                ("lf_file.rs", LineEnding::Unix),
                ("no_newline.rs", LineEnding::Unix),
            ],
        ),
        (
            "enforce_crlf",
            Some(LineEndingSetting::EnforceCrlf),
            [
                ("crlf_file.rs", LineEnding::Windows),
                ("lf_file.rs", LineEnding::Windows),
                ("no_newline.rs", LineEnding::Windows),
            ],
        ),
    ];

    for (case_name, line_ending_setting, expected_line_endings) in cases {
        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(
            path!("/dir"),
            json!({
                "crlf_file.rs": "one\r\ntwo\r\nthree\r\n",
                "lf_file.rs": "one\ntwo\nthree\n",
                "no_newline.rs": "single line",
            }),
        )
        .await;

        let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;
        let language_registry = project.read_with(cx, |project, _| project.languages().clone());
        language_registry.add(rust_lang());
        let worktree_id = project.update(cx, |project, cx| {
            project.worktrees(cx).next().unwrap().read(cx).id()
        });

        cx.update(|cx| {
            SettingsStore::update_global(cx, |store, cx| {
                store.update_user_settings(cx, |settings| {
                    settings.project.all_languages.defaults.line_ending = line_ending_setting;
                });
            });
        });
        cx.executor().run_until_parked();

        assert_line_endings_after_format(
            cx,
            &project,
            worktree_id,
            case_name,
            &expected_line_endings,
        )
        .await;
    }
}

#[gpui::test]
async fn test_line_ending_editorconfig_on_format_and_save(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let cases = [
        (
            "editorconfig lf",
            "lf",
            "crlf_file.rs",
            LineEnding::Windows,
            [
                ("crlf_file.rs", LineEnding::Unix),
                ("lf_file.rs", LineEnding::Unix),
                ("no_newline.rs", LineEnding::Unix),
            ],
            "one\ntwo\nthree\n",
        ),
        (
            "editorconfig crlf",
            "crlf",
            "lf_file.rs",
            LineEnding::Unix,
            [
                ("crlf_file.rs", LineEnding::Windows),
                ("lf_file.rs", LineEnding::Windows),
                ("no_newline.rs", LineEnding::Windows),
            ],
            "one\r\ntwo\r\nthree\r\n",
        ),
    ];

    for (
        case_name,
        editorconfig_end_of_line,
        buffer_path,
        initial_line_ending,
        expected_line_endings,
        expected_saved_contents,
    ) in cases
    {
        let file_system = FakeFs::new(cx.executor());
        file_system
            .insert_tree(
                path!("/dir"),
                json!({
                    ".editorconfig": format!("root = true\n[*.rs]\nend_of_line = {editorconfig_end_of_line}\n"),
                    "crlf_file.rs": "one\r\ntwo\r\nthree\r\n",
                    "lf_file.rs": "one\ntwo\nthree\n",
                    "no_newline.rs": "single line",
                }),
            )
            .await;

        let project = Project::test(file_system.clone(), [path!("/dir").as_ref()], cx).await;
        let language_registry = project.read_with(cx, |project, _| project.languages().clone());
        language_registry.add(rust_lang());
        cx.executor().run_until_parked();
        let worktree_id = project.update(cx, |project, cx| {
            project.worktrees(cx).next().unwrap().read(cx).id()
        });

        let buffer = project
            .update(cx, |project, cx| {
                project.open_buffer((worktree_id, rel_path(buffer_path)), cx)
            })
            .await
            .unwrap();
        buffer.update(cx, |buffer, _| {
            assert_eq!(buffer.line_ending(), initial_line_ending);
        });

        assert_line_endings_after_format(
            cx,
            &project,
            worktree_id,
            case_name,
            &expected_line_endings,
        )
        .await;

        project
            .update(cx, |project, cx| project.save_buffer(buffer, cx))
            .await
            .unwrap();
        let saved_path = PathBuf::from(path!("/dir")).join(buffer_path);
        assert_eq!(
            file_system.load(&saved_path).await.unwrap(),
            expected_saved_contents,
        );
    }
}

#[gpui::test]
async fn test_line_ending_initialization_for_new_buffers(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let cases = [
        (Some(LineEndingSetting::Detect), LineEnding::default()),
        (Some(LineEndingSetting::PreferLf), LineEnding::Unix),
        (Some(LineEndingSetting::PreferCrlf), LineEnding::Windows),
        (Some(LineEndingSetting::EnforceLf), LineEnding::Unix),
        (Some(LineEndingSetting::EnforceCrlf), LineEnding::Windows),
    ];

    for (line_ending_setting, expected_line_ending) in cases {
        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(path!("/dir"), json!({})).await;

        let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;
        cx.update(|cx| {
            SettingsStore::update_global(cx, |store, cx| {
                store.update_user_settings(cx, |settings| {
                    settings.project.all_languages.defaults.line_ending = line_ending_setting;
                });
            });
        });
        cx.executor().run_until_parked();

        let created_buffer = project
            .update(cx, |project, cx| project.create_buffer(None, false, cx))
            .unwrap()
            .await;
        created_buffer.update(cx, |buffer, _| {
            assert_eq!(buffer.line_ending(), expected_line_ending);
        });

        let local_buffer = project.update(cx, |project, cx| {
            project.create_local_buffer("single line", None, false, cx)
        });
        local_buffer.update(cx, |buffer, _| {
            assert_eq!(buffer.line_ending(), expected_line_ending);
        });

        let opened_missing_buffer = project
            .update(cx, |project, cx| {
                project.open_local_buffer(path!("/dir/new_file.rs"), cx)
            })
            .await
            .unwrap();
        opened_missing_buffer.update(cx, |buffer, _| {
            assert_eq!(buffer.line_ending(), expected_line_ending);
        });
    }
}

async fn assert_line_endings_after_format(
    cx: &mut gpui::TestAppContext,
    project: &Entity<Project>,
    worktree_id: WorktreeId,
    case_name: &str,
    expected_line_endings: &[(&str, LineEnding)],
) {
    for (path, expected_line_ending) in expected_line_endings {
        let buffer = project
            .update(cx, |project, cx| {
                project.open_buffer((worktree_id, rel_path(path)), cx)
            })
            .await
            .unwrap();
        let mut buffers = HashSet::default();
        buffers.insert(buffer.clone());
        project
            .update(cx, |project, cx| {
                project.format(
                    buffers,
                    project::FormatTarget::Buffers,
                    false,
                    project::FormatTrigger::Save,
                    cx,
                )
            })
            .await
            .unwrap();
        buffer.update(cx, |buffer, _| {
            assert_eq!(
                buffer.line_ending(),
                *expected_line_ending,
                "unexpected line ending for {path} in {case_name}"
            );
        });
    }
}

// Regression test for https://github.com/zed-industries/zed/issues/59077:
// a "rename symbol" whose workspace edit also renames the file used to swap the
// two files' contents. The edited content must end up in the renamed file, and
// the open buffer must follow the rename regardless of the order the filesystem
// watcher reports the change (hence the seed iterations).
#[gpui::test]
async fn test_search(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/dir"),
        json!({
            "one.rs": "const ONE: usize = 1;",
            "two.rs": "const TWO: usize = one::ONE + one::ONE;",
            "three.rs": "const THREE: usize = one::ONE + two::TWO;",
            "four.rs": "const FOUR: usize = one::ONE + three::THREE;",
        }),
    )
    .await;
    let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;
    assert_eq!(
        search(
            &project,
            SearchQuery::text(
                "TWO",
                false,
                true,
                false,
                Default::default(),
                Default::default(),
                false,
                None
            )
            .unwrap(),
            cx
        )
        .await
        .unwrap(),
        HashMap::from_iter([
            (path!("dir/two.rs").to_string(), vec![6..9]),
            (path!("dir/three.rs").to_string(), vec![37..40])
        ])
    );

    let buffer_4 = project
        .update(cx, |project, cx| {
            project.open_local_buffer(path!("/dir/four.rs"), cx)
        })
        .await
        .unwrap();
    buffer_4.update(cx, |buffer, cx| {
        let text = "two::TWO";
        buffer.edit([(20..28, text), (31..43, text)], None, cx);
    });

    assert_eq!(
        search(
            &project,
            SearchQuery::text(
                "TWO",
                false,
                true,
                false,
                Default::default(),
                Default::default(),
                false,
                None,
            )
            .unwrap(),
            cx
        )
        .await
        .unwrap(),
        HashMap::from_iter([
            (path!("dir/two.rs").to_string(), vec![6..9]),
            (path!("dir/three.rs").to_string(), vec![37..40]),
            (path!("dir/four.rs").to_string(), vec![25..28, 36..39])
        ])
    );
}

#[gpui::test]
async fn test_search_multiline_crlf(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/dir"),
        json!({
            "crlf.rs": "alpha\r\nbeta\r\ngamma",
            "lf.rs": "alpha\nbeta\ngamma",
        }),
    )
    .await;
    let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;

    // A query with CRLF line endings should match buffers regardless of their
    // on-disk line endings (buffers always store normalized `\n`).
    assert_eq!(
        search(
            &project,
            SearchQuery::text(
                "alpha\r\nbeta",
                false,
                true,
                false,
                Default::default(),
                Default::default(),
                false,
                None,
            )
            .unwrap(),
            cx
        )
        .await
        .unwrap(),
        HashMap::from_iter([
            (path!("dir/crlf.rs").to_string(), vec![0..10]),
            (path!("dir/lf.rs").to_string(), vec![0..10]),
        ])
    );

    // The reverse: a query with LF line endings should match a file that is
    // stored with CRLF line endings on disk.
    assert_eq!(
        search(
            &project,
            SearchQuery::text(
                "alpha\nbeta",
                false,
                true,
                false,
                Default::default(),
                Default::default(),
                false,
                None,
            )
            .unwrap(),
            cx
        )
        .await
        .unwrap(),
        HashMap::from_iter([
            (path!("dir/crlf.rs").to_string(), vec![0..10]),
            (path!("dir/lf.rs").to_string(), vec![0..10]),
        ])
    );
}

#[gpui::test]
async fn test_search_with_inclusions(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let search_query = "file";

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/dir"),
        json!({
            "one.rs": r#"// Rust file one"#,
            "one.ts": r#"// TypeScript file one"#,
            "two.rs": r#"// Rust file two"#,
            "two.ts": r#"// TypeScript file two"#,
        }),
    )
    .await;
    let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;

    assert!(
        search(
            &project,
            SearchQuery::text(
                search_query,
                false,
                true,
                false,
                PathMatcher::new(&["*.odd".to_owned()], PathStyle::local()).unwrap(),
                Default::default(),
                false,
                None
            )
            .unwrap(),
            cx
        )
        .await
        .unwrap()
        .is_empty(),
        "If no inclusions match, no files should be returned"
    );

    assert_eq!(
        search(
            &project,
            SearchQuery::text(
                search_query,
                false,
                true,
                false,
                PathMatcher::new(&["*.rs".to_owned()], PathStyle::local()).unwrap(),
                Default::default(),
                false,
                None
            )
            .unwrap(),
            cx
        )
        .await
        .unwrap(),
        HashMap::from_iter([
            (path!("dir/one.rs").to_string(), vec![8..12]),
            (path!("dir/two.rs").to_string(), vec![8..12]),
        ]),
        "Rust only search should give only Rust files"
    );

    assert_eq!(
        search(
            &project,
            SearchQuery::text(
                search_query,
                false,
                true,
                false,
                PathMatcher::new(&["*.ts".to_owned(), "*.odd".to_owned()], PathStyle::local())
                    .unwrap(),
                Default::default(),
                false,
                None,
            )
            .unwrap(),
            cx
        )
        .await
        .unwrap(),
        HashMap::from_iter([
            (path!("dir/one.ts").to_string(), vec![14..18]),
            (path!("dir/two.ts").to_string(), vec![14..18]),
        ]),
        "TypeScript only search should give only TypeScript files, even if other inclusions don't match anything"
    );

    assert_eq!(
        search(
            &project,
            SearchQuery::text(
                search_query,
                false,
                true,
                false,
                PathMatcher::new(
                    &["*.rs".to_owned(), "*.ts".to_owned(), "*.odd".to_owned()],
                    PathStyle::local()
                )
                .unwrap(),
                Default::default(),
                false,
                None,
            )
            .unwrap(),
            cx
        )
        .await
        .unwrap(),
        HashMap::from_iter([
            (path!("dir/two.ts").to_string(), vec![14..18]),
            (path!("dir/one.rs").to_string(), vec![8..12]),
            (path!("dir/one.ts").to_string(), vec![14..18]),
            (path!("dir/two.rs").to_string(), vec![8..12]),
        ]),
        "Rust and typescript search should give both Rust and TypeScript files, even if other inclusions don't match anything"
    );
}

#[gpui::test]
async fn test_search_with_exclusions(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let search_query = "file";

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/dir"),
        json!({
            "one.rs": r#"// Rust file one"#,
            "one.ts": r#"// TypeScript file one"#,
            "two.rs": r#"// Rust file two"#,
            "two.ts": r#"// TypeScript file two"#,
        }),
    )
    .await;
    let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;

    assert_eq!(
        search(
            &project,
            SearchQuery::text(
                search_query,
                false,
                true,
                false,
                Default::default(),
                PathMatcher::new(&["*.odd".to_owned()], PathStyle::local()).unwrap(),
                false,
                None,
            )
            .unwrap(),
            cx
        )
        .await
        .unwrap(),
        HashMap::from_iter([
            (path!("dir/one.rs").to_string(), vec![8..12]),
            (path!("dir/one.ts").to_string(), vec![14..18]),
            (path!("dir/two.rs").to_string(), vec![8..12]),
            (path!("dir/two.ts").to_string(), vec![14..18]),
        ]),
        "If no exclusions match, all files should be returned"
    );

    assert_eq!(
        search(
            &project,
            SearchQuery::text(
                search_query,
                false,
                true,
                false,
                Default::default(),
                PathMatcher::new(&["*.rs".to_owned()], PathStyle::local()).unwrap(),
                false,
                None,
            )
            .unwrap(),
            cx
        )
        .await
        .unwrap(),
        HashMap::from_iter([
            (path!("dir/one.ts").to_string(), vec![14..18]),
            (path!("dir/two.ts").to_string(), vec![14..18]),
        ]),
        "Rust exclusion search should give only TypeScript files"
    );

    assert_eq!(
        search(
            &project,
            SearchQuery::text(
                search_query,
                false,
                true,
                false,
                Default::default(),
                PathMatcher::new(&["*.ts".to_owned(), "*.odd".to_owned()], PathStyle::local())
                    .unwrap(),
                false,
                None,
            )
            .unwrap(),
            cx
        )
        .await
        .unwrap(),
        HashMap::from_iter([
            (path!("dir/one.rs").to_string(), vec![8..12]),
            (path!("dir/two.rs").to_string(), vec![8..12]),
        ]),
        "TypeScript exclusion search should give only Rust files, even if other exclusions don't match anything"
    );

    assert!(
        search(
            &project,
            SearchQuery::text(
                search_query,
                false,
                true,
                false,
                Default::default(),
                PathMatcher::new(
                    &["*.rs".to_owned(), "*.ts".to_owned(), "*.odd".to_owned()],
                    PathStyle::local(),
                )
                .unwrap(),
                false,
                None,
            )
            .unwrap(),
            cx
        )
        .await
        .unwrap()
        .is_empty(),
        "Rust and typescript exclusion should give no files, even if other exclusions don't match anything"
    );
}

#[gpui::test]
async fn test_search_with_buffer_exclusions(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let search_query = "file";

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/dir"),
        json!({
            "one.rs": r#"// Rust file one"#,
            "one.ts": r#"// TypeScript file one"#,
            "two.rs": r#"// Rust file two"#,
            "two.ts": r#"// TypeScript file two"#,
        }),
    )
    .await;

    let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;
    let path_style = PathStyle::local();
    let _buffer = project.update(cx, |project, cx| {
        project.create_local_buffer("file", None, false, cx)
    });

    assert_eq!(
        search(
            &project,
            SearchQuery::text(
                search_query,
                false,
                true,
                false,
                Default::default(),
                PathMatcher::new(&["*.odd".to_owned()], path_style).unwrap(),
                false,
                None,
            )
            .unwrap(),
            cx
        )
        .await
        .unwrap(),
        HashMap::from_iter([
            (path!("dir/one.rs").to_string(), vec![8..12]),
            (path!("dir/one.ts").to_string(), vec![14..18]),
            (path!("dir/two.rs").to_string(), vec![8..12]),
            (path!("dir/two.ts").to_string(), vec![14..18]),
        ]),
        "If no exclusions match, all files should be returned"
    );

    assert_eq!(
        search(
            &project,
            SearchQuery::text(
                search_query,
                false,
                true,
                false,
                Default::default(),
                PathMatcher::new(&["*.rs".to_owned()], path_style).unwrap(),
                false,
                None,
            )
            .unwrap(),
            cx
        )
        .await
        .unwrap(),
        HashMap::from_iter([
            (path!("dir/one.ts").to_string(), vec![14..18]),
            (path!("dir/two.ts").to_string(), vec![14..18]),
        ]),
        "Rust exclusion search should give only TypeScript files"
    );

    assert_eq!(
        search(
            &project,
            SearchQuery::text(
                search_query,
                false,
                true,
                false,
                Default::default(),
                PathMatcher::new(&["*.ts".to_owned(), "*.odd".to_owned()], path_style).unwrap(),
                false,
                None,
            )
            .unwrap(),
            cx
        )
        .await
        .unwrap(),
        HashMap::from_iter([
            (path!("dir/one.rs").to_string(), vec![8..12]),
            (path!("dir/two.rs").to_string(), vec![8..12]),
        ]),
        "TypeScript exclusion search should give only Rust files, even if other exclusions don't match anything"
    );

    assert!(
        search(
            &project,
            SearchQuery::text(
                search_query,
                false,
                true,
                false,
                Default::default(),
                PathMatcher::new(
                    &["*.rs".to_owned(), "*.ts".to_owned(), "*.odd".to_owned()],
                    PathStyle::local(),
                )
                .unwrap(),
                false,
                None,
            )
            .unwrap(),
            cx
        )
        .await
        .unwrap()
        .is_empty(),
        "Rust and typescript exclusion should give no files, even if other exclusions don't match anything"
    );
}

#[gpui::test]
async fn test_search_with_exclusions_and_inclusions(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let search_query = "file";

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/dir"),
        json!({
            "one.rs": r#"// Rust file one"#,
            "one.ts": r#"// TypeScript file one"#,
            "two.rs": r#"// Rust file two"#,
            "two.ts": r#"// TypeScript file two"#,
        }),
    )
    .await;
    let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;
    assert!(
        search(
            &project,
            SearchQuery::text(
                search_query,
                false,
                true,
                false,
                PathMatcher::new(&["*.odd".to_owned()], PathStyle::local()).unwrap(),
                PathMatcher::new(&["*.odd".to_owned()], PathStyle::local()).unwrap(),
                false,
                None,
            )
            .unwrap(),
            cx
        )
        .await
        .unwrap()
        .is_empty(),
        "If both no exclusions and inclusions match, exclusions should win and return nothing"
    );

    assert!(
        search(
            &project,
            SearchQuery::text(
                search_query,
                false,
                true,
                false,
                PathMatcher::new(&["*.ts".to_owned()], PathStyle::local()).unwrap(),
                PathMatcher::new(&["*.ts".to_owned()], PathStyle::local()).unwrap(),
                false,
                None,
            )
            .unwrap(),
            cx
        )
        .await
        .unwrap()
        .is_empty(),
        "If both TypeScript exclusions and inclusions match, exclusions should win and return nothing files."
    );

    assert!(
        search(
            &project,
            SearchQuery::text(
                search_query,
                false,
                true,
                false,
                PathMatcher::new(&["*.ts".to_owned(), "*.odd".to_owned()], PathStyle::local())
                    .unwrap(),
                PathMatcher::new(&["*.ts".to_owned(), "*.odd".to_owned()], PathStyle::local())
                    .unwrap(),
                false,
                None,
            )
            .unwrap(),
            cx
        )
        .await
        .unwrap()
        .is_empty(),
        "Non-matching inclusions and exclusions should not change that."
    );

    assert_eq!(
        search(
            &project,
            SearchQuery::text(
                search_query,
                false,
                true,
                false,
                PathMatcher::new(&["*.ts".to_owned(), "*.odd".to_owned()], PathStyle::local())
                    .unwrap(),
                PathMatcher::new(&["*.rs".to_owned(), "*.odd".to_owned()], PathStyle::local())
                    .unwrap(),
                false,
                None,
            )
            .unwrap(),
            cx
        )
        .await
        .unwrap(),
        HashMap::from_iter([
            (path!("dir/one.ts").to_string(), vec![14..18]),
            (path!("dir/two.ts").to_string(), vec![14..18]),
        ]),
        "Non-intersecting TypeScript inclusions and Rust exclusions should return TypeScript files"
    );
}

#[gpui::test]
async fn test_search_multiple_worktrees_with_inclusions(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/worktree-a"),
        json!({
            "haystack.rs": r#"// NEEDLE"#,
            "haystack.ts": r#"// NEEDLE"#,
        }),
    )
    .await;
    fs.insert_tree(
        path!("/worktree-b"),
        json!({
            "haystack.rs": r#"// NEEDLE"#,
            "haystack.ts": r#"// NEEDLE"#,
        }),
    )
    .await;

    let path_style = PathStyle::local();
    let project = Project::test(
        fs.clone(),
        [path!("/worktree-a").as_ref(), path!("/worktree-b").as_ref()],
        cx,
    )
    .await;

    assert_eq!(
        search(
            &project,
            SearchQuery::text(
                "NEEDLE",
                false,
                true,
                false,
                PathMatcher::new(&["worktree-a/*.rs".to_owned()], path_style).unwrap(),
                Default::default(),
                true,
                None,
            )
            .unwrap(),
            cx
        )
        .await
        .unwrap(),
        HashMap::from_iter([(path!("worktree-a/haystack.rs").to_string(), vec![3..9])]),
        "should only return results from included worktree"
    );
    assert_eq!(
        search(
            &project,
            SearchQuery::text(
                "NEEDLE",
                false,
                true,
                false,
                PathMatcher::new(&["worktree-b/*.rs".to_owned()], path_style).unwrap(),
                Default::default(),
                true,
                None,
            )
            .unwrap(),
            cx
        )
        .await
        .unwrap(),
        HashMap::from_iter([(path!("worktree-b/haystack.rs").to_string(), vec![3..9])]),
        "should only return results from included worktree"
    );

    assert_eq!(
        search(
            &project,
            SearchQuery::text(
                "NEEDLE",
                false,
                true,
                false,
                PathMatcher::new(&["*.ts".to_owned()], path_style).unwrap(),
                Default::default(),
                false,
                None,
            )
            .unwrap(),
            cx
        )
        .await
        .unwrap(),
        HashMap::from_iter([
            (path!("worktree-a/haystack.ts").to_string(), vec![3..9]),
            (path!("worktree-b/haystack.ts").to_string(), vec![3..9])
        ]),
        "should return results from both worktrees"
    );
}

#[gpui::test]
async fn test_search_in_gitignored_dirs(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        path!("/dir"),
        json!({
            ".git": {},
            ".gitignore": "**/target\n/node_modules\n",
            "target": {
                "index.txt": "index_key:index_value"
            },
            "node_modules": {
                "eslint": {
                    "index.ts": "const eslint_key = 'eslint value'",
                    "package.json": r#"{ "some_key": "some value" }"#,
                },
                "prettier": {
                    "index.ts": "const prettier_key = 'prettier value'",
                    "package.json": r#"{ "other_key": "other value" }"#,
                },
            },
            "package.json": r#"{ "main_key": "main value" }"#,
        }),
    )
    .await;
    let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;

    let query = "key";
    assert_eq!(
        search(
            &project,
            SearchQuery::text(
                query,
                false,
                false,
                false,
                Default::default(),
                Default::default(),
                false,
                None,
            )
            .unwrap(),
            cx
        )
        .await
        .unwrap(),
        HashMap::from_iter([(path!("dir/package.json").to_string(), vec![8..11])]),
        "Only one non-ignored file should have the query"
    );

    let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;
    let path_style = PathStyle::local();
    assert_eq!(
        search(
            &project,
            SearchQuery::text(
                query,
                false,
                false,
                true,
                Default::default(),
                Default::default(),
                false,
                None,
            )
            .unwrap(),
            cx
        )
        .await
        .unwrap(),
        HashMap::from_iter([
            (path!("dir/package.json").to_string(), vec![8..11]),
            (path!("dir/target/index.txt").to_string(), vec![6..9]),
            (
                path!("dir/node_modules/prettier/package.json").to_string(),
                vec![9..12]
            ),
            (
                path!("dir/node_modules/prettier/index.ts").to_string(),
                vec![15..18]
            ),
            (
                path!("dir/node_modules/eslint/index.ts").to_string(),
                vec![13..16]
            ),
            (
                path!("dir/node_modules/eslint/package.json").to_string(),
                vec![8..11]
            ),
        ]),
        "Unrestricted search with ignored directories should find every file with the query"
    );

    let files_to_include =
        PathMatcher::new(&["node_modules/prettier/**".to_owned()], path_style).unwrap();
    let files_to_exclude = PathMatcher::new(&["*.ts".to_owned()], path_style).unwrap();
    let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;
    assert_eq!(
        search(
            &project,
            SearchQuery::text(
                query,
                false,
                false,
                true,
                files_to_include,
                files_to_exclude,
                false,
                None,
            )
            .unwrap(),
            cx
        )
        .await
        .unwrap(),
        HashMap::from_iter([(
            path!("dir/node_modules/prettier/package.json").to_string(),
            vec![9..12]
        )]),
        "With search including ignored prettier directory and excluding TS files, only one file should be found"
    );
}

#[gpui::test]
async fn test_visibility_for_paths_in_gitignored_dirs(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/dir"),
        json!({
            ".git": {},
            ".gitignore": ".checkouts/\n",
            "src": {
                "main.rs": "fn main() {}"
            },
            ".checkouts": {
                "worktrees": {
                    "foo": {
                        "README.md": "hello"
                    }
                }
            },
        }),
    )
    .await;
    let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;
    cx.run_until_parked();

    project.read_with(cx, |project, cx| {
        let root = PathBuf::from(path!("/dir"));
        let file = PathBuf::from(path!("/dir/src/main.rs"));
        let ignored_dir = PathBuf::from(path!("/dir/.checkouts/worktrees/foo"));
        let file_in_ignored_dir = PathBuf::from(path!("/dir/.checkouts/worktrees/foo/README.md"));

        assert_eq!(
            project.visibility_for_paths(std::slice::from_ref(&root), true, cx),
            Some(true)
        );
        assert_eq!(
            project.visibility_for_paths(std::slice::from_ref(&file), true, cx),
            Some(true)
        );
        assert_eq!(
            project.visibility_for_subpaths(std::slice::from_ref(&file), cx),
            Some(true)
        );

        // Paths inside gitignored directories aren't scanned, so they aren't
        // considered contained by the project; opening one should behave like
        // opening an unrelated path.
        for path in [&ignored_dir, &file_in_ignored_dir] {
            assert_eq!(
                project.visibility_for_paths(std::slice::from_ref(path), true, cx),
                None,
                "{path:?} should not be considered contained"
            );
            assert_eq!(
                project.visibility_for_paths(std::slice::from_ref(path), false, cx),
                None,
                "{path:?} should not be considered contained"
            );
            assert_eq!(
                project.visibility_for_subpaths(std::slice::from_ref(path), cx),
                None,
                "{path:?} should not be considered a contained subpath"
            );
        }
    });
}

#[gpui::test]
async fn test_search_with_unicode(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/dir"),
        json!({
            "one.rs": "// ПРИВЕТ? привет!",
            "two.rs": "// ПРИВЕТ.",
            "three.rs": "// привет",
        }),
    )
    .await;
    let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;
    let unicode_case_sensitive_query = SearchQuery::text(
        "привет",
        false,
        true,
        false,
        Default::default(),
        Default::default(),
        false,
        None,
    );
    assert_matches!(unicode_case_sensitive_query, Ok(SearchQuery::Text { .. }));
    assert_eq!(
        search(&project, unicode_case_sensitive_query.unwrap(), cx)
            .await
            .unwrap(),
        HashMap::from_iter([
            (path!("dir/one.rs").to_string(), vec![17..29]),
            (path!("dir/three.rs").to_string(), vec![3..15]),
        ])
    );

    let unicode_case_insensitive_query = SearchQuery::text(
        "привет",
        false,
        false,
        false,
        Default::default(),
        Default::default(),
        false,
        None,
    );
    assert_matches!(
        unicode_case_insensitive_query,
        Ok(SearchQuery::Regex { .. })
    );
    assert_eq!(
        search(&project, unicode_case_insensitive_query.unwrap(), cx)
            .await
            .unwrap(),
        HashMap::from_iter([
            (path!("dir/one.rs").to_string(), vec![3..15, 17..29]),
            (path!("dir/two.rs").to_string(), vec![3..15]),
            (path!("dir/three.rs").to_string(), vec![3..15]),
        ])
    );

    assert_eq!(
        search(
            &project,
            SearchQuery::text(
                "привет.",
                false,
                false,
                false,
                Default::default(),
                Default::default(),
                false,
                None,
            )
            .unwrap(),
            cx
        )
        .await
        .unwrap(),
        HashMap::from_iter([(path!("dir/two.rs").to_string(), vec![3..16]),])
    );
}

#[gpui::test]
async fn test_search_in_unopened_non_utf8_files(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let text = "// 你好世界 hello\n// 这是一个中文注释，包含很多汉字，用来帮助编码检测器正确判断文件编码。\n// 编码检测需要足够多的中文内容才能可靠工作。\n";

    let gb2312 = encoding_rs::Encoding::for_label(b"gb2312").unwrap();
    assert_eq!(gb2312, encoding_rs::GBK);
    let (gbk_bytes, _, had_errors) = gb2312.encode(text);
    assert!(!had_errors);

    let korean_text = "// 안녕하세요 세계 hello\n// 이것은 한국어 주석입니다. 인코딩 감지기가 파일 인코딩을 올바르게 판단할 수 있도록 충분히 많은 한글 내용을 담고 있습니다.\n// 인코딩 감지는 충분한 한국어 내용이 있어야 안정적으로 작동합니다.\n";
    let (euc_kr_bytes, _, had_errors) = encoding_rs::EUC_KR.encode(korean_text);
    assert!(!had_errors);

    let mut utf16_bytes = vec![0xFF, 0xFE];
    utf16_bytes.extend(text.encode_utf16().flat_map(|u| u.to_le_bytes()));

    let mut binary_bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    binary_bytes.extend_from_slice(text.as_bytes());

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/dir"),
        json!({
            "utf8.rs": text,
        }),
    )
    .await;
    fs.insert_file(path!("/dir/gbk.rs"), gbk_bytes.into_owned())
        .await;
    fs.insert_file(path!("/dir/euc_kr.rs"), euc_kr_bytes.into_owned())
        .await;
    fs.insert_file(path!("/dir/utf16.rs"), utf16_bytes).await;
    fs.insert_file(path!("/dir/binary.dat"), binary_bytes).await;

    let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;

    assert_eq!(
        search(
            &project,
            SearchQuery::text(
                "世界",
                false,
                true,
                false,
                PathMatcher::default(),
                PathMatcher::default(),
                false,
                None,
            )
            .unwrap(),
            cx
        )
        .await
        .unwrap(),
        HashMap::from_iter([
            (path!("dir/utf8.rs").to_string(), vec![9..15]),
            (path!("dir/gbk.rs").to_string(), vec![9..15]),
            (path!("dir/utf16.rs").to_string(), vec![9..15]),
        ])
    );

    assert_eq!(
        search(
            &project,
            SearchQuery::text(
                "HELLO",
                false,
                false,
                false,
                PathMatcher::default(),
                PathMatcher::default(),
                false,
                None,
            )
            .unwrap(),
            cx
        )
        .await
        .unwrap(),
        HashMap::from_iter([
            (path!("dir/utf8.rs").to_string(), vec![16..21]),
            (path!("dir/gbk.rs").to_string(), vec![16..21]),
            (path!("dir/euc_kr.rs").to_string(), vec![26..31]),
            (path!("dir/utf16.rs").to_string(), vec![16..21]),
        ])
    );

    assert_eq!(
        search(
            &project,
            SearchQuery::text(
                "세계",
                false,
                true,
                false,
                PathMatcher::default(),
                PathMatcher::default(),
                false,
                None,
            )
            .unwrap(),
            cx
        )
        .await
        .unwrap(),
        HashMap::from_iter([(path!("dir/euc_kr.rs").to_string(), vec![19..25])])
    );
}

#[gpui::test]
async fn test_create_entry(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        "/one/two",
        json!({
            "three": {
                "a.txt": "",
                "four": {}
            },
            "c.rs": ""
        }),
    )
    .await;

    let project = Project::test(fs.clone(), ["/one/two/three".as_ref()], cx).await;
    project
        .update(cx, |project, cx| {
            let id = project.worktrees(cx).next().unwrap().read(cx).id();
            project.create_entry((id, rel_path("b..")), true, cx)
        })
        .await
        .unwrap()
        .into_included()
        .unwrap();

    assert_eq!(
        fs.paths(true),
        vec![
            PathBuf::from(path!("/")),
            PathBuf::from(path!("/one")),
            PathBuf::from(path!("/one/two")),
            PathBuf::from(path!("/one/two/c.rs")),
            PathBuf::from(path!("/one/two/three")),
            PathBuf::from(path!("/one/two/three/a.txt")),
            PathBuf::from(path!("/one/two/three/b..")),
            PathBuf::from(path!("/one/two/three/four")),
        ]
    );
}

#[gpui::test]
async fn test_reordering_worktrees(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        "/dir",
        json!({
            "a.rs": "let a = 1;",
            "b.rs": "let b = 2;",
            "c.rs": "let c = 2;",
        }),
    )
    .await;

    let project = Project::test(
        fs,
        [
            "/dir/a.rs".as_ref(),
            "/dir/b.rs".as_ref(),
            "/dir/c.rs".as_ref(),
        ],
        cx,
    )
    .await;

    // check the initial state and get the worktrees
    let (worktree_a, worktree_b, worktree_c) = project.update(cx, |project, cx| {
        let worktrees = project.visible_worktrees(cx).collect::<Vec<_>>();
        assert_eq!(worktrees.len(), 3);

        let worktree_a = worktrees[0].read(cx);
        let worktree_b = worktrees[1].read(cx);
        let worktree_c = worktrees[2].read(cx);

        // check they start in the right order
        assert_eq!(worktree_a.abs_path().to_str().unwrap(), "/dir/a.rs");
        assert_eq!(worktree_b.abs_path().to_str().unwrap(), "/dir/b.rs");
        assert_eq!(worktree_c.abs_path().to_str().unwrap(), "/dir/c.rs");

        (
            worktrees[0].clone(),
            worktrees[1].clone(),
            worktrees[2].clone(),
        )
    });

    // move first worktree to after the second
    // [a, b, c] -> [b, a, c]
    project
        .update(cx, |project, cx| {
            let first = worktree_a.read(cx);
            let second = worktree_b.read(cx);
            project.move_worktree(first.id(), second.id(), cx)
        })
        .expect("moving first after second");

    // check the state after moving
    project.update(cx, |project, cx| {
        let worktrees = project.visible_worktrees(cx).collect::<Vec<_>>();
        assert_eq!(worktrees.len(), 3);

        let first = worktrees[0].read(cx);
        let second = worktrees[1].read(cx);
        let third = worktrees[2].read(cx);

        // check they are now in the right order
        assert_eq!(first.abs_path().to_str().unwrap(), "/dir/b.rs");
        assert_eq!(second.abs_path().to_str().unwrap(), "/dir/a.rs");
        assert_eq!(third.abs_path().to_str().unwrap(), "/dir/c.rs");
    });

    // move the second worktree to before the first
    // [b, a, c] -> [a, b, c]
    project
        .update(cx, |project, cx| {
            let second = worktree_a.read(cx);
            let first = worktree_b.read(cx);
            project.move_worktree(first.id(), second.id(), cx)
        })
        .expect("moving second before first");

    // check the state after moving
    project.update(cx, |project, cx| {
        let worktrees = project.visible_worktrees(cx).collect::<Vec<_>>();
        assert_eq!(worktrees.len(), 3);

        let first = worktrees[0].read(cx);
        let second = worktrees[1].read(cx);
        let third = worktrees[2].read(cx);

        // check they are now in the right order
        assert_eq!(first.abs_path().to_str().unwrap(), "/dir/a.rs");
        assert_eq!(second.abs_path().to_str().unwrap(), "/dir/b.rs");
        assert_eq!(third.abs_path().to_str().unwrap(), "/dir/c.rs");
    });

    // move the second worktree to after the third
    // [a, b, c] -> [a, c, b]
    project
        .update(cx, |project, cx| {
            let second = worktree_b.read(cx);
            let third = worktree_c.read(cx);
            project.move_worktree(second.id(), third.id(), cx)
        })
        .expect("moving second after third");

    // check the state after moving
    project.update(cx, |project, cx| {
        let worktrees = project.visible_worktrees(cx).collect::<Vec<_>>();
        assert_eq!(worktrees.len(), 3);

        let first = worktrees[0].read(cx);
        let second = worktrees[1].read(cx);
        let third = worktrees[2].read(cx);

        // check they are now in the right order
        assert_eq!(first.abs_path().to_str().unwrap(), "/dir/a.rs");
        assert_eq!(second.abs_path().to_str().unwrap(), "/dir/c.rs");
        assert_eq!(third.abs_path().to_str().unwrap(), "/dir/b.rs");
    });

    // move the third worktree to before the second
    // [a, c, b] -> [a, b, c]
    project
        .update(cx, |project, cx| {
            let third = worktree_c.read(cx);
            let second = worktree_b.read(cx);
            project.move_worktree(third.id(), second.id(), cx)
        })
        .expect("moving third before second");

    // check the state after moving
    project.update(cx, |project, cx| {
        let worktrees = project.visible_worktrees(cx).collect::<Vec<_>>();
        assert_eq!(worktrees.len(), 3);

        let first = worktrees[0].read(cx);
        let second = worktrees[1].read(cx);
        let third = worktrees[2].read(cx);

        // check they are now in the right order
        assert_eq!(first.abs_path().to_str().unwrap(), "/dir/a.rs");
        assert_eq!(second.abs_path().to_str().unwrap(), "/dir/b.rs");
        assert_eq!(third.abs_path().to_str().unwrap(), "/dir/c.rs");
    });

    // move the first worktree to after the third
    // [a, b, c] -> [b, c, a]
    project
        .update(cx, |project, cx| {
            let first = worktree_a.read(cx);
            let third = worktree_c.read(cx);
            project.move_worktree(first.id(), third.id(), cx)
        })
        .expect("moving first after third");

    // check the state after moving
    project.update(cx, |project, cx| {
        let worktrees = project.visible_worktrees(cx).collect::<Vec<_>>();
        assert_eq!(worktrees.len(), 3);

        let first = worktrees[0].read(cx);
        let second = worktrees[1].read(cx);
        let third = worktrees[2].read(cx);

        // check they are now in the right order
        assert_eq!(first.abs_path().to_str().unwrap(), "/dir/b.rs");
        assert_eq!(second.abs_path().to_str().unwrap(), "/dir/c.rs");
        assert_eq!(third.abs_path().to_str().unwrap(), "/dir/a.rs");
    });

    // move the third worktree to before the first
    // [b, c, a] -> [a, b, c]
    project
        .update(cx, |project, cx| {
            let third = worktree_a.read(cx);
            let first = worktree_b.read(cx);
            project.move_worktree(third.id(), first.id(), cx)
        })
        .expect("moving third before first");

    // check the state after moving
    project.update(cx, |project, cx| {
        let worktrees = project.visible_worktrees(cx).collect::<Vec<_>>();
        assert_eq!(worktrees.len(), 3);

        let first = worktrees[0].read(cx);
        let second = worktrees[1].read(cx);
        let third = worktrees[2].read(cx);

        // check they are now in the right order
        assert_eq!(first.abs_path().to_str().unwrap(), "/dir/a.rs");
        assert_eq!(second.abs_path().to_str().unwrap(), "/dir/b.rs");
        assert_eq!(third.abs_path().to_str().unwrap(), "/dir/c.rs");
    });
}

#[gpui::test]
async fn test_unstaged_diff_for_buffer(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let staged_contents = r#"
        fn main() {
            println!("hello world");
        }
    "#
    .unindent();
    let file_contents = r#"
        // print goodbye
        fn main() {
            println!("goodbye world");
        }
    "#
    .unindent();

    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        "/dir",
        json!({
            ".git": {},
           "src": {
               "main.rs": file_contents,
           }
        }),
    )
    .await;

    fs.set_index_for_repo(Path::new("/dir/.git"), &[("src/main.rs", staged_contents)]);

    let project = Project::test(fs.clone(), ["/dir".as_ref()], cx).await;

    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer("/dir/src/main.rs", cx)
        })
        .await
        .unwrap();
    let unstaged_diff = project
        .update(cx, |project, cx| {
            project.open_unstaged_diff(buffer.clone(), cx)
        })
        .await
        .unwrap();

    cx.run_until_parked();
    unstaged_diff.update(cx, |unstaged_diff, cx| {
        let snapshot = buffer.read(cx).snapshot();
        assert_hunks(
            unstaged_diff.snapshot(cx).hunks(&snapshot),
            &snapshot,
            &unstaged_diff.base_text_string(cx).unwrap(),
            &[
                (0..1, "", "// print goodbye\n", DiffHunkStatus::added_none()),
                (
                    2..3,
                    "    println!(\"hello world\");\n",
                    "    println!(\"goodbye world\");\n",
                    DiffHunkStatus::modified_none(),
                ),
            ],
        );
    });

    let staged_contents = r#"
        // print goodbye
        fn main() {
        }
    "#
    .unindent();

    fs.set_index_for_repo(Path::new("/dir/.git"), &[("src/main.rs", staged_contents)]);

    cx.run_until_parked();
    unstaged_diff.update(cx, |unstaged_diff, cx| {
        let snapshot = buffer.read(cx).snapshot();
        assert_hunks(
            unstaged_diff.snapshot(cx).hunks_intersecting_range(
                Anchor::min_max_range_for_buffer(snapshot.remote_id()),
                &snapshot,
            ),
            &snapshot,
            &unstaged_diff.base_text(cx).text(),
            &[(
                2..3,
                "",
                "    println!(\"goodbye world\");\n",
                DiffHunkStatus::added_none(),
            )],
        );
    });
}

#[gpui::test]
async fn test_reopening_unstaged_diff_after_drop(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let staged_contents = r#"
        fn main() {
            println!("hello world");
        }
    "#
    .unindent();
    let file_contents = r#"
        // print goodbye
        fn main() {
            println!("goodbye world");
        }
    "#
    .unindent();

    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        "/dir",
        json!({
            ".git": {},
           "src": {
               "main.rs": file_contents,
           }
        }),
    )
    .await;
    fs.set_index_for_repo(Path::new("/dir/.git"), &[("src/main.rs", staged_contents)]);

    let project = Project::test(fs.clone(), ["/dir".as_ref()], cx).await;
    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer("/dir/src/main.rs", cx)
        })
        .await
        .unwrap();
    let buffer_id = buffer.read_with(cx, |buffer, _| buffer.remote_id());

    let unstaged_diff = project
        .update(cx, |project, cx| {
            project.open_unstaged_diff(buffer.clone(), cx)
        })
        .await
        .unwrap();
    cx.run_until_parked();

    // Drop the diff while the buffer (and its git state) stays alive.
    drop(unstaged_diff);
    cx.run_until_parked();
    project.read_with(cx, |project, cx| {
        assert!(
            project
                .git_store()
                .read(cx)
                .get_unstaged_diff(buffer_id, cx)
                .is_none(),
            "unstaged diff should have been released"
        );
    });

    // Reopen the diff. The new entity must be registered in the git store,
    // and its hunks must be recalculated.
    let unstaged_diff = project
        .update(cx, |project, cx| {
            project.open_unstaged_diff(buffer.clone(), cx)
        })
        .await
        .unwrap();
    cx.run_until_parked();

    project.read_with(cx, |project, cx| {
        let registered = project
            .git_store()
            .read(cx)
            .get_unstaged_diff(buffer_id, cx);
        assert_eq!(
            registered.as_ref(),
            Some(&unstaged_diff),
            "reopened unstaged diff should be registered in the git store"
        );
    });

    unstaged_diff.update(cx, |unstaged_diff, cx| {
        let snapshot = buffer.read(cx).snapshot();
        assert_hunks(
            unstaged_diff.snapshot(cx).hunks(&snapshot),
            &snapshot,
            &unstaged_diff.base_text_string(cx).unwrap(),
            &[
                (0..1, "", "// print goodbye\n", DiffHunkStatus::added_none()),
                (
                    2..3,
                    "    println!(\"hello world\");\n",
                    "    println!(\"goodbye world\");\n",
                    DiffHunkStatus::modified_none(),
                ),
            ],
        );
    });
}

#[gpui::test]
async fn test_staged_diff_for_buffer(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let committed_contents = r#"
        fn main() {
            println!("hello world");
        }
    "#
    .unindent();
    let staged_contents = r#"
        // print goodbye
        fn main() {
            println!("goodbye world");
        }
    "#
    .unindent();
    let file_contents = r#"
        // print goodbye
        fn main() {
            println!("working copy only");
        }
    "#
    .unindent();

    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        "/dir",
        json!({
            ".git": {},
           "src": {
               "main.rs": file_contents,
           }
        }),
    )
    .await;
    fs.set_head_for_repo(
        Path::new("/dir/.git"),
        &[("src/main.rs", committed_contents)],
        "deadbeef",
    );
    fs.set_index_for_repo(Path::new("/dir/.git"), &[("src/main.rs", staged_contents)]);

    let project = Project::test(fs.clone(), ["/dir".as_ref()], cx).await;
    let language_registry = project.read_with(cx, |project, _| project.languages().clone());
    let language = rust_lang();
    language_registry.add(language.clone());

    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer("/dir/src/main.rs", cx)
        })
        .await
        .unwrap();
    let unstaged_diff = project
        .update(cx, |project, cx| {
            project.open_unstaged_diff(buffer.clone(), cx)
        })
        .await
        .unwrap();
    cx.run_until_parked();
    unstaged_diff.read_with(cx, |diff, cx| {
        assert_eq!(
            diff.base_text(cx).language().cloned(),
            Some(language.clone())
        );
    });

    let (staged_diff, index_text_buffer) = project
        .update(cx, |project, cx| {
            project.open_staged_diff(buffer.clone(), cx)
        })
        .await
        .unwrap();

    cx.run_until_parked();
    index_text_buffer.read_with(cx, |buffer, cx| {
        let file = buffer.file().unwrap();
        assert_eq!(file.file_name(cx), "main.rs");
        assert_eq!(
            file.disk_state(),
            DiskState::Historic { was_deleted: false }
        );
    });
    unstaged_diff.read_with(cx, |diff, cx| {
        assert_eq!(
            diff.base_text(cx).language().cloned(),
            Some(language.clone())
        );
    });
    staged_diff.update(cx, |staged_diff, cx| {
        let snapshot = staged_diff.snapshot(cx);
        let buffer_snapshot = snapshot.buffer_snapshot();
        assert_hunks(
            snapshot.hunks(buffer_snapshot),
            buffer_snapshot,
            &staged_diff.base_text_string(cx).unwrap(),
            &[
                (0..1, "", "// print goodbye\n", DiffHunkStatus::added_none()),
                (
                    2..3,
                    "    println!(\"hello world\");\n",
                    "    println!(\"goodbye world\");\n",
                    DiffHunkStatus::modified_none(),
                ),
            ],
        );
    });

    let staged_contents = r#"
        // print goodbye
        fn main() {
        }
    "#
    .unindent();
    fs.set_index_for_repo(Path::new("/dir/.git"), &[("src/main.rs", staged_contents)]);

    cx.run_until_parked();
    staged_diff.update(cx, |staged_diff, cx| {
        let snapshot = staged_diff.snapshot(cx);
        let buffer_snapshot = snapshot.buffer_snapshot();
        assert_hunks(
            snapshot.hunks(buffer_snapshot),
            buffer_snapshot,
            &staged_diff.base_text_string(cx).unwrap(),
            &[
                (0..1, "", "// print goodbye\n", DiffHunkStatus::added_none()),
                (
                    2..2,
                    "    println!(\"hello world\");\n",
                    "",
                    DiffHunkStatus::deleted_none(),
                ),
            ],
        );
    });
}

#[gpui::test]
async fn test_base_text_buffers_released_when_diffs_dropped(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let committed_contents = "one\ntwo\nthree\n";
    let staged_contents = "one\nTWO\nthree\n";
    let file_contents = "one\nTWO\nTHREE\n";

    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        "/dir",
        json!({
            ".git": {},
            "src": {
                "main.rs": file_contents,
            }
        }),
    )
    .await;
    fs.set_head_for_repo(
        Path::new("/dir/.git"),
        &[("src/main.rs", committed_contents.to_owned())],
        "deadbeef",
    );
    fs.set_index_for_repo(
        Path::new("/dir/.git"),
        &[("src/main.rs", staged_contents.to_owned())],
    );

    let project = Project::test(fs.clone(), ["/dir".as_ref()], cx).await;
    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer("/dir/src/main.rs", cx)
        })
        .await
        .unwrap();

    let uncommitted_diff = project
        .update(cx, |project, cx| {
            project.open_uncommitted_diff(buffer.clone(), cx)
        })
        .await
        .unwrap();
    let unstaged_diff = project
        .update(cx, |project, cx| {
            project.open_unstaged_diff(buffer.clone(), cx)
        })
        .await
        .unwrap();
    cx.run_until_parked();

    let weak_head_text_buffer =
        uncommitted_diff.read_with(cx, |diff, _| diff.base_text_buffer().downgrade());
    let weak_index_text_buffer =
        unstaged_diff.read_with(cx, |diff, _| diff.base_text_buffer().downgrade());

    drop(uncommitted_diff);
    cx.run_until_parked();
    cx.update(|_| {});
    weak_head_text_buffer.assert_released();
    assert!(
        weak_index_text_buffer.upgrade().is_some(),
        "index text buffer should stay alive while the unstaged diff is open"
    );

    drop(unstaged_diff);
    cx.run_until_parked();
    cx.update(|_| {});
    weak_index_text_buffer.assert_released();

    let uncommitted_diff = project
        .update(cx, |project, cx| {
            project.open_uncommitted_diff(buffer.clone(), cx)
        })
        .await
        .unwrap();
    cx.run_until_parked();
    uncommitted_diff.update(cx, |uncommitted_diff, cx| {
        assert_eq!(
            uncommitted_diff.base_text_string(cx).as_deref(),
            Some(committed_contents),
        );
        let snapshot = buffer.read(cx).snapshot();
        assert_hunks(
            uncommitted_diff.snapshot(cx).hunks(&snapshot),
            &snapshot,
            &uncommitted_diff.base_text_string(cx).unwrap(),
            &[(
                1..3,
                "two\nthree\n",
                "TWO\nTHREE\n",
                DiffHunkStatus::modified(DiffHunkSecondaryStatus::OverlapsWithSecondaryHunk),
            )],
        );
    });
}

#[gpui::test]
async fn test_staged_diff_without_unstaged_diff(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let committed_contents = "one\ntwo\nthree\n";
    let staged_contents = "one\nTWO\nthree\n";
    let file_contents = "one\nTWO\nthree\n";

    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        "/dir",
        json!({
            ".git": {},
            "src": {
                "main.rs": file_contents,
            }
        }),
    )
    .await;
    fs.set_head_for_repo(
        Path::new("/dir/.git"),
        &[("src/main.rs", committed_contents.to_owned())],
        "deadbeef",
    );
    fs.set_index_for_repo(
        Path::new("/dir/.git"),
        &[("src/main.rs", staged_contents.to_owned())],
    );

    let project = Project::test(fs.clone(), ["/dir".as_ref()], cx).await;
    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer("/dir/src/main.rs", cx)
        })
        .await
        .unwrap();

    let (staged_diff, _) = project
        .update(cx, |project, cx| {
            project.open_staged_diff(buffer.clone(), cx)
        })
        .await
        .unwrap();
    cx.run_until_parked();

    staged_diff.update(cx, |staged_diff, cx| {
        let snapshot = staged_diff.snapshot(cx);
        let buffer_snapshot = snapshot.buffer_snapshot();
        assert_hunks(
            snapshot.hunks(buffer_snapshot),
            buffer_snapshot,
            &staged_diff.base_text_string(cx).unwrap(),
            &[(1..2, "two\n", "TWO\n", DiffHunkStatus::modified_none())],
        );
    });

    fs.set_index_for_repo(
        Path::new("/dir/.git"),
        &[("src/main.rs", "one\nTWO\nTHREE\n".to_owned())],
    );
    cx.run_until_parked();

    staged_diff.update(cx, |staged_diff, cx| {
        let snapshot = staged_diff.snapshot(cx);
        let buffer_snapshot = snapshot.buffer_snapshot();
        assert_hunks(
            snapshot.hunks(buffer_snapshot),
            buffer_snapshot,
            &staged_diff.base_text_string(cx).unwrap(),
            &[(
                1..3,
                "two\nthree\n",
                "TWO\nTHREE\n",
                DiffHunkStatus::modified_none(),
            )],
        );
    });
}

#[gpui::test]
async fn test_uncommitted_diff_for_buffer(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let committed_contents = r#"
        fn main() {
            println!("hello world");
        }
    "#
    .unindent();
    let staged_contents = r#"
        fn main() {
            println!("goodbye world");
        }
    "#
    .unindent();
    let file_contents = r#"
        // print goodbye
        fn main() {
            println!("goodbye world");
        }
    "#
    .unindent();

    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        "/dir",
        json!({
            ".git": {},
           "src": {
               "modification.rs": file_contents,
           }
        }),
    )
    .await;

    fs.set_head_for_repo(
        Path::new("/dir/.git"),
        &[
            ("src/modification.rs", committed_contents),
            ("src/deletion.rs", "// the-deleted-contents\n".into()),
        ],
        "deadbeef",
    );
    fs.set_index_for_repo(
        Path::new("/dir/.git"),
        &[
            ("src/modification.rs", staged_contents),
            ("src/deletion.rs", "// the-deleted-contents\n".into()),
        ],
    );

    let project = Project::test(fs.clone(), ["/dir".as_ref()], cx).await;
    let language_registry = project.read_with(cx, |project, _| project.languages().clone());
    let language = rust_lang();
    language_registry.add(language.clone());

    let buffer_1 = project
        .update(cx, |project, cx| {
            project.open_local_buffer("/dir/src/modification.rs", cx)
        })
        .await
        .unwrap();
    let diff_1 = project
        .update(cx, |project, cx| {
            project.open_uncommitted_diff(buffer_1.clone(), cx)
        })
        .await
        .unwrap();
    diff_1.read_with(cx, |diff, cx| {
        assert_eq!(diff.base_text(cx).language().cloned(), Some(language))
    });
    cx.run_until_parked();
    diff_1.update(cx, |diff, cx| {
        let snapshot = buffer_1.read(cx).snapshot();
        assert_hunks(
            diff.snapshot(cx).hunks_intersecting_range(
                Anchor::min_max_range_for_buffer(snapshot.remote_id()),
                &snapshot,
            ),
            &snapshot,
            &diff.base_text_string(cx).unwrap(),
            &[
                (
                    0..1,
                    "",
                    "// print goodbye\n",
                    DiffHunkStatus::added(DiffHunkSecondaryStatus::HasSecondaryHunk),
                ),
                (
                    2..3,
                    "    println!(\"hello world\");\n",
                    "    println!(\"goodbye world\");\n",
                    DiffHunkStatus::modified_none(),
                ),
            ],
        );
    });

    // Reset HEAD to a version that differs from both the buffer and the index.
    let committed_contents = r#"
        // print goodbye
        fn main() {
        }
    "#
    .unindent();
    fs.set_head_for_repo(
        Path::new("/dir/.git"),
        &[
            ("src/modification.rs", committed_contents.clone()),
            ("src/deletion.rs", "// the-deleted-contents\n".into()),
        ],
        "deadbeef",
    );

    // Buffer now has an unstaged hunk.
    cx.run_until_parked();
    diff_1.update(cx, |diff, cx| {
        let snapshot = buffer_1.read(cx).snapshot();
        assert_hunks(
            diff.snapshot(cx).hunks_intersecting_range(
                Anchor::min_max_range_for_buffer(snapshot.remote_id()),
                &snapshot,
            ),
            &snapshot,
            &diff.base_text(cx).text(),
            &[(
                2..3,
                "",
                "    println!(\"goodbye world\");\n",
                DiffHunkStatus::added_none(),
            )],
        );
    });

    // Open a buffer for a file that's been deleted.
    let buffer_2 = project
        .update(cx, |project, cx| {
            project.open_local_buffer("/dir/src/deletion.rs", cx)
        })
        .await
        .unwrap();
    let diff_2 = project
        .update(cx, |project, cx| {
            project.open_uncommitted_diff(buffer_2.clone(), cx)
        })
        .await
        .unwrap();
    cx.run_until_parked();
    diff_2.update(cx, |diff, cx| {
        let snapshot = buffer_2.read(cx).snapshot();
        assert_hunks(
            diff.snapshot(cx).hunks_intersecting_range(
                Anchor::min_max_range_for_buffer(snapshot.remote_id()),
                &snapshot,
            ),
            &snapshot,
            &diff.base_text_string(cx).unwrap(),
            &[(
                0..0,
                "// the-deleted-contents\n",
                "",
                DiffHunkStatus::deleted(DiffHunkSecondaryStatus::HasSecondaryHunk),
            )],
        );
    });

    // Stage the deletion of this file
    fs.set_index_for_repo(
        Path::new("/dir/.git"),
        &[("src/modification.rs", committed_contents.clone())],
    );
    cx.run_until_parked();
    diff_2.update(cx, |diff, cx| {
        let snapshot = buffer_2.read(cx).snapshot();
        assert_hunks(
            diff.snapshot(cx).hunks_intersecting_range(
                Anchor::min_max_range_for_buffer(snapshot.remote_id()),
                &snapshot,
            ),
            &snapshot,
            &diff.base_text_string(cx).unwrap(),
            &[(
                0..0,
                "// the-deleted-contents\n",
                "",
                DiffHunkStatus::deleted(DiffHunkSecondaryStatus::NoSecondaryHunk),
            )],
        );
    });
}

#[gpui::test]
async fn test_staging_hunks(cx: &mut gpui::TestAppContext) {
    use DiffHunkSecondaryStatus::*;
    init_test(cx);

    let committed_contents = r#"
        zero
        one
        two
        three
        four
        five
    "#
    .unindent();
    let file_contents = r#"
        one
        TWO
        three
        FOUR
        five
    "#
    .unindent();

    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        "/dir",
        json!({
            ".git": {},
            "file.txt": file_contents.clone()
        }),
    )
    .await;

    fs.set_head_and_index_for_repo(
        path!("/dir/.git").as_ref(),
        &[("file.txt", committed_contents.clone())],
    );

    let project = Project::test(fs.clone(), ["/dir".as_ref()], cx).await;

    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer("/dir/file.txt", cx)
        })
        .await
        .unwrap();
    let snapshot = buffer.read_with(cx, |buffer, _| buffer.snapshot());
    let uncommitted_diff = project
        .update(cx, |project, cx| {
            project.open_uncommitted_diff(buffer.clone(), cx)
        })
        .await
        .unwrap();

    // The hunks are initially unstaged.
    uncommitted_diff.read_with(cx, |diff, cx| {
        assert_hunks(
            diff.snapshot(cx).hunks(&snapshot),
            &snapshot,
            &diff.base_text_string(cx).unwrap(),
            &[
                (
                    0..0,
                    "zero\n",
                    "",
                    DiffHunkStatus::deleted(HasSecondaryHunk),
                ),
                (
                    1..2,
                    "two\n",
                    "TWO\n",
                    DiffHunkStatus::modified(HasSecondaryHunk),
                ),
                (
                    3..4,
                    "four\n",
                    "FOUR\n",
                    DiffHunkStatus::modified(HasSecondaryHunk),
                ),
            ],
        );
    });

    // Stage a hunk. It appears as optimistically staged.
    let range = snapshot.anchor_before(Point::new(1, 0))..snapshot.anchor_before(Point::new(2, 0));
    project
        .update(cx, |project, cx| {
            let unstaged_diff = uncommitted_diff.read(cx).secondary_diff().unwrap();
            project.stage_hunks(buffer.clone(), unstaged_diff, vec![range], cx)
        })
        .unwrap();
    uncommitted_diff.read_with(cx, |diff, cx| {
        assert_hunks(
            diff.snapshot(cx).hunks(&snapshot),
            &snapshot,
            &diff.base_text_string(cx).unwrap(),
            &[
                (
                    0..0,
                    "zero\n",
                    "",
                    DiffHunkStatus::deleted(HasSecondaryHunk),
                ),
                (
                    1..2,
                    "two\n",
                    "TWO\n",
                    DiffHunkStatus::modified(SecondaryHunkRemovalPending),
                ),
                (
                    3..4,
                    "four\n",
                    "FOUR\n",
                    DiffHunkStatus::modified(HasSecondaryHunk),
                ),
            ],
        );
    });

    // When the write to the index completes, it appears as staged.
    cx.run_until_parked();
    uncommitted_diff.read_with(cx, |diff, cx| {
        assert_hunks(
            diff.snapshot(cx).hunks(&snapshot),
            &snapshot,
            &diff.base_text_string(cx).unwrap(),
            &[
                (
                    0..0,
                    "zero\n",
                    "",
                    DiffHunkStatus::deleted(HasSecondaryHunk),
                ),
                (
                    1..2,
                    "two\n",
                    "TWO\n",
                    DiffHunkStatus::modified(NoSecondaryHunk),
                ),
                (
                    3..4,
                    "four\n",
                    "FOUR\n",
                    DiffHunkStatus::modified(HasSecondaryHunk),
                ),
            ],
        );
    });

    // Simulate a problem writing to the git index.
    fs.set_error_message_for_index_write(
        "/dir/.git".as_ref(),
        Some("failed to write git index".into()),
    );

    // Stage another hunk.
    let range = snapshot.anchor_before(Point::new(3, 0))..snapshot.anchor_before(Point::new(4, 0));
    project
        .update(cx, |project, cx| {
            let unstaged_diff = uncommitted_diff.read(cx).secondary_diff().unwrap();
            project.stage_hunks(buffer.clone(), unstaged_diff, vec![range], cx)
        })
        .unwrap();
    uncommitted_diff.read_with(cx, |diff, cx| {
        assert_hunks(
            diff.snapshot(cx).hunks(&snapshot),
            &snapshot,
            &diff.base_text_string(cx).unwrap(),
            &[
                (
                    0..0,
                    "zero\n",
                    "",
                    DiffHunkStatus::deleted(HasSecondaryHunk),
                ),
                (
                    1..2,
                    "two\n",
                    "TWO\n",
                    DiffHunkStatus::modified(NoSecondaryHunk),
                ),
                (
                    3..4,
                    "four\n",
                    "FOUR\n",
                    DiffHunkStatus::modified(SecondaryHunkRemovalPending),
                ),
            ],
        );
    });

    // When the write fails, the hunk returns to being unstaged.
    cx.run_until_parked();
    uncommitted_diff.read_with(cx, |diff, cx| {
        assert_hunks(
            diff.snapshot(cx).hunks(&snapshot),
            &snapshot,
            &diff.base_text_string(cx).unwrap(),
            &[
                (
                    0..0,
                    "zero\n",
                    "",
                    DiffHunkStatus::deleted(HasSecondaryHunk),
                ),
                (
                    1..2,
                    "two\n",
                    "TWO\n",
                    DiffHunkStatus::modified(NoSecondaryHunk),
                ),
                (
                    3..4,
                    "four\n",
                    "FOUR\n",
                    DiffHunkStatus::modified(HasSecondaryHunk),
                ),
            ],
        );
    });

    // Allow writing to the git index to succeed again.
    fs.set_error_message_for_index_write("/dir/.git".as_ref(), None);

    // Stage two hunks with separate operations.
    let (range0, range2) = uncommitted_diff.read_with(cx, |diff, cx| {
        let hunks = diff.snapshot(cx).hunks(&snapshot).collect::<Vec<_>>();
        (hunks[0].buffer_range.clone(), hunks[2].buffer_range.clone())
    });
    project
        .update(cx, |project, cx| {
            let unstaged_diff = uncommitted_diff.read(cx).secondary_diff().unwrap();
            project.stage_hunks(buffer.clone(), unstaged_diff, vec![range0], cx)
        })
        .unwrap();
    project
        .update(cx, |project, cx| {
            let unstaged_diff = uncommitted_diff.read(cx).secondary_diff().unwrap();
            project.stage_hunks(buffer.clone(), unstaged_diff, vec![range2], cx)
        })
        .unwrap();

    // Both staged hunks appear as pending.
    uncommitted_diff.read_with(cx, |diff, cx| {
        assert_hunks(
            diff.snapshot(cx).hunks(&snapshot),
            &snapshot,
            &diff.base_text_string(cx).unwrap(),
            &[
                (
                    0..0,
                    "zero\n",
                    "",
                    DiffHunkStatus::deleted(SecondaryHunkRemovalPending),
                ),
                (
                    1..2,
                    "two\n",
                    "TWO\n",
                    DiffHunkStatus::modified(NoSecondaryHunk),
                ),
                (
                    3..4,
                    "four\n",
                    "FOUR\n",
                    DiffHunkStatus::modified(SecondaryHunkRemovalPending),
                ),
            ],
        );
    });

    // Both staging operations take effect.
    cx.run_until_parked();
    uncommitted_diff.read_with(cx, |diff, cx| {
        assert_hunks(
            diff.snapshot(cx).hunks(&snapshot),
            &snapshot,
            &diff.base_text_string(cx).unwrap(),
            &[
                (0..0, "zero\n", "", DiffHunkStatus::deleted(NoSecondaryHunk)),
                (
                    1..2,
                    "two\n",
                    "TWO\n",
                    DiffHunkStatus::modified(NoSecondaryHunk),
                ),
                (
                    3..4,
                    "four\n",
                    "FOUR\n",
                    DiffHunkStatus::modified(NoSecondaryHunk),
                ),
            ],
        );
    });
}

#[gpui::test(iterations = 10)]
async fn test_uncommitted_diff_opened_before_unstaged_diff(cx: &mut gpui::TestAppContext) {
    use DiffHunkSecondaryStatus::*;
    init_test(cx);

    let committed_contents = "one\ntwo\nthree\n";
    let file_contents = "one\nTWO\nthree\n";

    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        "/dir",
        json!({
            ".git": {},
            "file.txt": file_contents,
        }),
    )
    .await;
    fs.set_head_and_index_for_repo(
        path!("/dir/.git").as_ref(),
        &[("file.txt", committed_contents.into())],
    );

    let project = Project::test(fs.clone(), ["/dir".as_ref()], cx).await;
    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer("/dir/file.txt", cx)
        })
        .await
        .unwrap();

    let uncommitted_diff_task = project.update(cx, |project, cx| {
        project.open_uncommitted_diff(buffer.clone(), cx)
    });
    let unstaged_diff_task = project.update(cx, |project, cx| {
        project.open_unstaged_diff(buffer.clone(), cx)
    });
    let (uncommitted_diff, unstaged_diff) =
        futures::future::join(uncommitted_diff_task, unstaged_diff_task).await;
    let uncommitted_diff = uncommitted_diff.unwrap();
    let unstaged_diff = unstaged_diff.unwrap();

    cx.run_until_parked();

    uncommitted_diff.read_with(cx, |diff, _| {
        assert_eq!(
            diff.secondary_diff(),
            Some(unstaged_diff.clone()),
            "the unstaged diff returned to callers should be the uncommitted diff's secondary"
        );
    });
    project.read_with(cx, |project, cx| {
        let buffer_id = buffer.read(cx).remote_id();
        assert_eq!(
            project
                .git_store()
                .read(cx)
                .get_unstaged_diff(buffer_id, cx),
            Some(unstaged_diff.clone()),
            "the unstaged diff returned to callers should be the registered one"
        );
    });

    uncommitted_diff.read_with(cx, |diff, cx| {
        let snapshot = buffer.read(cx).snapshot();
        assert_hunks(
            diff.snapshot(cx).hunks_intersecting_range(
                Anchor::min_max_range_for_buffer(snapshot.remote_id()),
                &snapshot,
            ),
            &snapshot,
            &diff.base_text_string(cx).unwrap(),
            &[(
                1..2,
                "two\n",
                "TWO\n",
                DiffHunkStatus::modified(HasSecondaryHunk),
            )],
        );
    });
}

#[gpui::test(seeds(340, 472))]
async fn test_staging_hunks_with_delayed_fs_event(cx: &mut gpui::TestAppContext) {
    use DiffHunkSecondaryStatus::*;
    init_test(cx);

    let committed_contents = r#"
        zero
        one
        two
        three
        four
        five
    "#
    .unindent();
    let file_contents = r#"
        one
        TWO
        three
        FOUR
        five
    "#
    .unindent();

    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        "/dir",
        json!({
            ".git": {},
            "file.txt": file_contents.clone()
        }),
    )
    .await;

    fs.set_head_for_repo(
        "/dir/.git".as_ref(),
        &[("file.txt", committed_contents.clone())],
        "deadbeef",
    );
    fs.set_index_for_repo(
        "/dir/.git".as_ref(),
        &[("file.txt", committed_contents.clone())],
    );

    let project = Project::test(fs.clone(), ["/dir".as_ref()], cx).await;

    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer("/dir/file.txt", cx)
        })
        .await
        .unwrap();
    let snapshot = buffer.read_with(cx, |buffer, _| buffer.snapshot());
    let uncommitted_diff = project
        .update(cx, |project, cx| {
            project.open_uncommitted_diff(buffer.clone(), cx)
        })
        .await
        .unwrap();

    // The hunks are initially unstaged.
    uncommitted_diff.read_with(cx, |diff, cx| {
        assert_hunks(
            diff.snapshot(cx).hunks(&snapshot),
            &snapshot,
            &diff.base_text_string(cx).unwrap(),
            &[
                (
                    0..0,
                    "zero\n",
                    "",
                    DiffHunkStatus::deleted(HasSecondaryHunk),
                ),
                (
                    1..2,
                    "two\n",
                    "TWO\n",
                    DiffHunkStatus::modified(HasSecondaryHunk),
                ),
                (
                    3..4,
                    "four\n",
                    "FOUR\n",
                    DiffHunkStatus::modified(HasSecondaryHunk),
                ),
            ],
        );
    });

    // Pause IO events
    fs.pause_events();

    // Stage the first hunk.
    let range = uncommitted_diff.read_with(cx, |diff, cx| {
        diff.snapshot(cx)
            .hunks(&snapshot)
            .next()
            .unwrap()
            .buffer_range
    });
    project
        .update(cx, |project, cx| {
            let unstaged_diff = uncommitted_diff.read(cx).secondary_diff().unwrap();
            project.stage_hunks(buffer.clone(), unstaged_diff, vec![range], cx)
        })
        .unwrap();
    uncommitted_diff.read_with(cx, |diff, cx| {
        assert_hunks(
            diff.snapshot(cx).hunks(&snapshot),
            &snapshot,
            &diff.base_text_string(cx).unwrap(),
            &[
                (
                    0..0,
                    "zero\n",
                    "",
                    DiffHunkStatus::deleted(SecondaryHunkRemovalPending),
                ),
                (
                    1..2,
                    "two\n",
                    "TWO\n",
                    DiffHunkStatus::modified(HasSecondaryHunk),
                ),
                (
                    3..4,
                    "four\n",
                    "FOUR\n",
                    DiffHunkStatus::modified(HasSecondaryHunk),
                ),
            ],
        );
    });

    // Stage the second hunk *before* receiving the FS event for the first hunk.
    cx.run_until_parked();
    let range = uncommitted_diff.read_with(cx, |diff, cx| {
        diff.snapshot(cx)
            .hunks(&snapshot)
            .nth(1)
            .unwrap()
            .buffer_range
    });
    project
        .update(cx, |project, cx| {
            let unstaged_diff = uncommitted_diff.read(cx).secondary_diff().unwrap();
            project.stage_hunks(buffer.clone(), unstaged_diff, vec![range], cx)
        })
        .unwrap();
    uncommitted_diff.read_with(cx, |diff, cx| {
        assert_hunks(
            diff.snapshot(cx).hunks(&snapshot),
            &snapshot,
            &diff.base_text_string(cx).unwrap(),
            &[
                (
                    0..0,
                    "zero\n",
                    "",
                    DiffHunkStatus::deleted(SecondaryHunkRemovalPending),
                ),
                (
                    1..2,
                    "two\n",
                    "TWO\n",
                    DiffHunkStatus::modified(SecondaryHunkRemovalPending),
                ),
                (
                    3..4,
                    "four\n",
                    "FOUR\n",
                    DiffHunkStatus::modified(HasSecondaryHunk),
                ),
            ],
        );
    });

    // Process the FS event for staging the first hunk (second event is still pending).
    fs.flush_events(1);
    cx.run_until_parked();

    // Stage the third hunk before receiving the second FS event.
    let range = uncommitted_diff.read_with(cx, |diff, cx| {
        diff.snapshot(cx)
            .hunks(&snapshot)
            .nth(2)
            .unwrap()
            .buffer_range
    });
    project
        .update(cx, |project, cx| {
            let unstaged_diff = uncommitted_diff.read(cx).secondary_diff().unwrap();
            project.stage_hunks(buffer.clone(), unstaged_diff, vec![range], cx)
        })
        .unwrap();

    // Wait for all remaining IO.
    cx.run_until_parked();
    fs.flush_events(fs.buffered_event_count());

    // Now all hunks are staged.
    cx.run_until_parked();
    uncommitted_diff.read_with(cx, |diff, cx| {
        assert_hunks(
            diff.snapshot(cx).hunks(&snapshot),
            &snapshot,
            &diff.base_text_string(cx).unwrap(),
            &[
                (0..0, "zero\n", "", DiffHunkStatus::deleted(NoSecondaryHunk)),
                (
                    1..2,
                    "two\n",
                    "TWO\n",
                    DiffHunkStatus::modified(NoSecondaryHunk),
                ),
                (
                    3..4,
                    "four\n",
                    "FOUR\n",
                    DiffHunkStatus::modified(NoSecondaryHunk),
                ),
            ],
        );
    });
}

#[gpui::test]
async fn test_restaging_hunk_after_optimistic_unstage(cx: &mut gpui::TestAppContext) {
    use DiffHunkSecondaryStatus::*;
    init_test(cx);

    let committed_contents = r#"
        one
        two
        three
    "#
    .unindent();
    let file_contents = r#"
        one
        TWO
        three
    "#
    .unindent();

    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        path!("/dir"),
        json!({
            ".git": {},
            "file.txt": file_contents.clone()
        }),
    )
    .await;

    // The hunk starts out staged: the index already matches the worktree.
    fs.set_head_for_repo(
        path!("/dir/.git").as_ref(),
        &[("file.txt", committed_contents.clone())],
        "deadbeef",
    );
    fs.set_index_for_repo(
        path!("/dir/.git").as_ref(),
        &[("file.txt", file_contents.clone())],
    );
    let repo = fs
        .open_repo(path!("/dir/.git").as_ref(), Some("git".as_ref()))
        .unwrap();

    let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;
    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer(path!("/dir/file.txt"), cx)
        })
        .await
        .unwrap();
    let snapshot = buffer.read_with(cx, |buffer, _| buffer.snapshot());
    let uncommitted_diff = project
        .update(cx, |project, cx| {
            project.open_uncommitted_diff(buffer.clone(), cx)
        })
        .await
        .unwrap();

    let hunk_range = uncommitted_diff.read_with(cx, |diff, cx| {
        assert_hunks(
            diff.snapshot(cx).hunks(&snapshot),
            &snapshot,
            &diff.base_text_string(cx).unwrap(),
            &[(
                1..2,
                "two\n",
                "TWO\n",
                DiffHunkStatus::modified(NoSecondaryHunk),
            )],
        );
        diff.snapshot(cx)
            .hunks(&snapshot)
            .next()
            .unwrap()
            .buffer_range
    });

    // Hold back FS events so that the index writes below stay un-reconciled:
    // the second operation must run while the first one's optimistic state is
    // still pending.
    fs.pause_events();

    // Unstage the hunk. It appears as optimistically unstaged.
    project
        .update(cx, |project, cx| {
            project.unstage_uncommitted_hunks(
                buffer.clone(),
                uncommitted_diff.clone(),
                vec![hunk_range.clone()],
                cx,
            )
        })
        .unwrap();
    uncommitted_diff.read_with(cx, |diff, cx| {
        assert_hunks(
            diff.snapshot(cx).hunks(&snapshot),
            &snapshot,
            &diff.base_text_string(cx).unwrap(),
            &[(
                1..2,
                "two\n",
                "TWO\n",
                DiffHunkStatus::modified(SecondaryHunkAdditionPending),
            )],
        );
    });

    // Re-stage the same hunk before the unstage write has been reconciled.
    project
        .update(cx, |project, cx| {
            let unstaged_diff = uncommitted_diff.read(cx).secondary_diff().unwrap();
            project.stage_hunks(buffer.clone(), unstaged_diff, vec![hunk_range.clone()], cx)
        })
        .unwrap();
    uncommitted_diff.read_with(cx, |diff, cx| {
        assert_hunks(
            diff.snapshot(cx).hunks(&snapshot),
            &snapshot,
            &diff.base_text_string(cx).unwrap(),
            &[(
                1..2,
                "two\n",
                "TWO\n",
                DiffHunkStatus::modified(SecondaryHunkRemovalPending),
            )],
        );
    });

    cx.run_until_parked();
    assert_eq!(
        repo.load_index_text(RepoPath::from_rel_path(rel_path("file.txt")))
            .await
            .unwrap(),
        file_contents.as_bytes(),
        "the re-stage should have overridden the in-flight unstage"
    );

    fs.unpause_events_and_flush();
    cx.run_until_parked();

    // Once everything settles, the hunk is staged again.
    uncommitted_diff.read_with(cx, |diff, cx| {
        assert_hunks(
            diff.snapshot(cx).hunks(&snapshot),
            &snapshot,
            &diff.base_text_string(cx).unwrap(),
            &[(
                1..2,
                "two\n",
                "TWO\n",
                DiffHunkStatus::modified(NoSecondaryHunk),
            )],
        );
    });
    assert_eq!(
        repo.load_index_text(RepoPath::from_rel_path(rel_path("file.txt")))
            .await
            .unwrap(),
        file_contents.as_bytes()
    );
}

#[gpui::test]
async fn test_staging_non_utf8_hunk_preserves_index_encoding(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let committed_text = "строка один\nстрока два\n";
    let file_text = "строка один\nстрока три\n";
    let (committed_bytes, _, _) = encoding_rs::WINDOWS_1251.encode(committed_text);
    let (file_bytes, _, _) = encoding_rs::WINDOWS_1251.encode(file_text);
    let committed_bytes = committed_bytes.into_owned();
    let file_bytes = file_bytes.into_owned();

    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(path!("/dir"), json!({ ".git": {} })).await;
    fs.insert_file(path!("/dir/file.txt"), file_bytes.clone())
        .await;
    fs.with_git_state(path!("/dir/.git").as_ref(), true, |state| {
        state
            .head_contents
            .insert(repo_path("file.txt"), committed_bytes.clone());
        state
            .index_contents
            .insert(repo_path("file.txt"), committed_bytes.clone());
        state.refs.insert("HEAD".into(), "deadbeef".into());
    })
    .unwrap();
    let repo = fs
        .open_repo(path!("/dir/.git").as_ref(), Some("git".as_ref()))
        .unwrap();

    let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;
    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer(path!("/dir/file.txt"), cx)
        })
        .await
        .unwrap();
    assert_eq!(
        buffer.read_with(cx, |buffer, _| buffer.encoding()),
        encoding_rs::WINDOWS_1251
    );

    let snapshot = buffer.read_with(cx, |buffer, _| buffer.snapshot());
    let uncommitted_diff = project
        .update(cx, |project, cx| {
            project.open_uncommitted_diff(buffer.clone(), cx)
        })
        .await
        .unwrap();
    let range = snapshot.anchor_before(Point::new(1, 0))..snapshot.anchor_before(Point::new(2, 0));

    project
        .update(cx, |project, cx| {
            let unstaged_diff = uncommitted_diff.read(cx).secondary_diff().unwrap();
            project.stage_hunks(buffer.clone(), unstaged_diff, vec![range], cx)
        })
        .unwrap();
    cx.run_until_parked();

    assert_eq!(
        repo.load_index_text(RepoPath::from_rel_path(rel_path("file.txt")))
            .await
            .unwrap(),
        file_bytes
    );
}

#[gpui::test(iterations = 25)]
async fn test_staging_random_hunks(
    mut rng: StdRng,
    _executor: BackgroundExecutor,
    cx: &mut gpui::TestAppContext,
) {
    let operations = env::var("OPERATIONS")
        .map(|i| i.parse().expect("invalid `OPERATIONS` variable"))
        .unwrap_or(20);

    use DiffHunkSecondaryStatus::*;
    init_test(cx);

    let committed_text = (0..40).map(|i| format!("line {i}\n")).collect::<String>();
    let index_text = committed_text.clone();
    let buffer_text = (0..40)
        .map(|i| match i {
            35 => format!("line {i}\ninserted after {i}\n"),
            _ if i < 30 && i % 5 == 0 => format!("line {i} (modified)\n"),
            _ => format!("line {i}\n"),
        })
        .collect::<String>();

    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        path!("/dir"),
        json!({
            ".git": {},
            "file.txt": buffer_text.clone()
        }),
    )
    .await;
    fs.set_head_for_repo(
        path!("/dir/.git").as_ref(),
        &[("file.txt", committed_text.clone())],
        "deadbeef",
    );
    fs.set_index_for_repo(
        path!("/dir/.git").as_ref(),
        &[("file.txt", index_text.clone())],
    );
    let repo = fs
        .open_repo(path!("/dir/.git").as_ref(), Some("git".as_ref()))
        .unwrap();

    let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;
    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer(path!("/dir/file.txt"), cx)
        })
        .await
        .unwrap();
    let snapshot = buffer.read_with(cx, |buffer, _| buffer.snapshot());
    let uncommitted_diff = project
        .update(cx, |project, cx| {
            project.open_uncommitted_diff(buffer.clone(), cx)
        })
        .await
        .unwrap();

    let mut hunks = uncommitted_diff.update(cx, |diff, cx| {
        diff.snapshot(cx).hunks(&snapshot).collect::<Vec<_>>()
    });
    assert_eq!(hunks.len(), 7);
    let insertion_hunk = hunks
        .iter()
        .find(|hunk| hunk.diff_base_byte_range.is_empty())
        .unwrap()
        .clone();
    fs.pause_events();
    project
        .update(cx, |project, cx| {
            let unstaged_diff = uncommitted_diff.read(cx).secondary_diff().unwrap();
            project.stage_hunks(
                buffer.clone(),
                unstaged_diff,
                vec![insertion_hunk.buffer_range.clone()],
                cx,
            )
        })
        .unwrap();
    project
        .update(cx, |project, cx| {
            project.unstage_uncommitted_hunks(
                buffer.clone(),
                uncommitted_diff.clone(),
                vec![insertion_hunk.buffer_range],
                cx,
            )
        })
        .unwrap();
    cx.executor().run_until_parked();
    assert_eq!(
        repo.load_index_text(RepoPath::from_rel_path(rel_path("file.txt")))
            .await
            .unwrap(),
        index_text.as_bytes()
    );
    fs.unpause_events_and_flush();
    cx.run_until_parked();
    hunks = uncommitted_diff.update(cx, |diff, cx| {
        diff.snapshot(cx).hunks(&snapshot).collect::<Vec<_>>()
    });
    assert_eq!(hunks.len(), 7);

    for _i in 0..operations {
        let hunk_ix = rng.random_range(0..hunks.len());
        let hunk = &mut hunks[hunk_ix];
        let row = hunk.range.start.row;

        let range = hunk.buffer_range.clone();
        if hunk.status().has_secondary_hunk() {
            log::info!("staging hunk at {row}");
            project
                .update(cx, |project, cx| {
                    let unstaged_diff = uncommitted_diff.read(cx).secondary_diff().unwrap();
                    project.stage_hunks(buffer.clone(), unstaged_diff, vec![range], cx)
                })
                .unwrap();
            hunk.secondary_status = SecondaryHunkRemovalPending;
        } else {
            log::info!("unstaging hunk at {row}");
            project
                .update(cx, |project, cx| {
                    project.unstage_uncommitted_hunks(
                        buffer.clone(),
                        uncommitted_diff.clone(),
                        vec![range],
                        cx,
                    )
                })
                .unwrap();
            hunk.secondary_status = SecondaryHunkAdditionPending;
        }

        for _ in 0..rng.random_range(0..10) {
            log::info!("yielding");
            cx.executor().simulate_random_delay().await;
        }
    }

    cx.executor().run_until_parked();

    for hunk in &mut hunks {
        if hunk.secondary_status == SecondaryHunkRemovalPending {
            hunk.secondary_status = NoSecondaryHunk;
        } else if hunk.secondary_status == SecondaryHunkAdditionPending {
            hunk.secondary_status = HasSecondaryHunk;
        }
    }

    log::info!(
        "index text:\n{}",
        String::from_utf8_lossy(
            &repo
                .load_index_text(RepoPath::from_rel_path(rel_path("file.txt")))
                .await
                .unwrap()
        )
    );

    uncommitted_diff.update(cx, |diff, cx| {
        let expected_hunks = hunks
            .iter()
            .map(|hunk| (hunk.range.start.row, hunk.secondary_status))
            .collect::<Vec<_>>();
        let actual_hunks = diff
            .snapshot(cx)
            .hunks(&snapshot)
            .map(|hunk| (hunk.range.start.row, hunk.secondary_status))
            .collect::<Vec<_>>();
        assert_eq!(actual_hunks, expected_hunks);
    });
}

#[gpui::test(iterations = 25)]
async fn test_staging_random_hunks_with_edits(
    mut rng: StdRng,
    _executor: BackgroundExecutor,
    cx: &mut gpui::TestAppContext,
) {
    let operations = env::var("OPERATIONS")
        .map(|i| i.parse().expect("invalid `OPERATIONS` variable"))
        .unwrap_or(30);

    init_test(cx);

    fn disk_index_text(fs: &FakeFs) -> String {
        let bytes = fs
            .with_git_state(path!("/dir/.git").as_ref(), false, |state| {
                state
                    .index_contents
                    .get(&repo_path("file.txt"))
                    .cloned()
                    .expect("file is always present in the index")
            })
            .unwrap();
        String::from_utf8(bytes).expect("test index contents are valid UTF-8")
    }

    fn random_row_range(
        snapshot: &text::BufferSnapshot,
        rng: &mut StdRng,
    ) -> (Range<Anchor>, Range<u32>) {
        let max_row = snapshot.max_point().row;
        let start_row = rng.random_range(0..=max_row);
        let end_row = rng.random_range(start_row..=max_row.min(start_row + 5));
        let start = Point::new(start_row, 0);
        let end = Point::new(end_row, snapshot.line_len(end_row));
        (
            snapshot.anchor_before(start)..snapshot.anchor_after(end),
            start_row..end_row,
        )
    }

    #[allow(clippy::type_complexity)]
    fn capture_diff_state(
        diff: &Entity<BufferDiff>,
        cx: &mut gpui::TestAppContext,
    ) -> (
        String,
        Option<String>,
        Vec<(Range<Point>, Range<usize>, DiffHunkSecondaryStatus)>,
    ) {
        diff.read_with(cx, |diff, cx| {
            let snapshot = diff.snapshot(cx);
            let buffer_snapshot = snapshot.buffer_snapshot().clone();
            let hunks = snapshot
                .hunks(&buffer_snapshot)
                .map(|hunk| {
                    (
                        hunk.range.clone(),
                        hunk.diff_base_byte_range.clone(),
                        hunk.secondary_status,
                    )
                })
                .collect();
            (buffer_snapshot.text(), snapshot.base_text_string(), hunks)
        })
    }

    fn assert_settled(diff: &Entity<BufferDiff>, name: &str, cx: &mut gpui::TestAppContext) {
        diff.read_with(cx, |diff, cx| {
            let snapshot = diff.snapshot(cx);
            let buffer_snapshot = snapshot.buffer_snapshot().clone();
            let cooked = snapshot
                .hunks(&buffer_snapshot)
                .map(|hunk| (hunk.range.clone(), hunk.secondary_status))
                .collect::<Vec<_>>();
            for (range, status) in &cooked {
                assert!(
                    !matches!(
                        status,
                        DiffHunkSecondaryStatus::SecondaryHunkAdditionPending
                            | DiffHunkSecondaryStatus::SecondaryHunkRemovalPending
                    ),
                    "{name} hunk at {range:?} still has pending status {status:?} after settling"
                );
            }
            let full_range = Anchor::min_max_range_for_buffer(buffer_snapshot.remote_id());
            let raw_ranges = snapshot
                .raw_hunks_intersecting_range(full_range, &buffer_snapshot)
                .map(|hunk| hunk.range)
                .collect::<Vec<_>>();
            let cooked_ranges = cooked
                .into_iter()
                .map(|(range, _)| range)
                .collect::<Vec<_>>();
            assert_eq!(
                raw_ranges, cooked_ranges,
                "{name} still has suppressed hunks after settling"
            );
        })
    }

    let mut committed_text = (0..40)
        .map(|i| match i {
            _ if i % 5 == 0 => format!("line {i} αβ🍐\n"),
            _ => format!("line {i}\n"),
        })
        .collect::<String>();
    let index_text = (0..40)
        .map(|i| match i {
            10 => "line 10 (modified ✅)\n".to_string(),
            20 => "line 20 (staged-only ⭐)\n".to_string(),
            _ if i % 5 == 0 => format!("line {i} αβ🍐\n"),
            _ => format!("line {i}\n"),
        })
        .collect::<String>();
    let buffer_text = (0..40)
        .map(|i| match i {
            35 => format!("line {i} αβ🍐\ninserted after {i}\n"),
            _ if i < 30 && i % 5 == 0 => format!("line {i} (modified ✅)\n"),
            _ if i % 5 == 0 => format!("line {i} αβ🍐\n"),
            _ => format!("line {i}\n"),
        })
        .collect::<String>();

    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        path!("/dir"),
        json!({
            ".git": {},
            "file.txt": buffer_text.clone()
        }),
    )
    .await;
    fs.set_head_for_repo(
        path!("/dir/.git").as_ref(),
        &[("file.txt", committed_text.clone())],
        "deadbeef",
    );
    fs.set_index_for_repo(path!("/dir/.git").as_ref(), &[("file.txt", index_text)]);
    let repo = fs
        .open_repo(path!("/dir/.git").as_ref(), Some("git".as_ref()))
        .unwrap();

    let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;
    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer(path!("/dir/file.txt"), cx)
        })
        .await
        .unwrap();
    let unstaged_diff = project
        .update(cx, |project, cx| {
            project.open_unstaged_diff(buffer.clone(), cx)
        })
        .await
        .unwrap();
    let (staged_diff, index_buffer) = project
        .update(cx, |project, cx| {
            project.open_staged_diff(buffer.clone(), cx)
        })
        .await
        .unwrap();
    let uncommitted_diff = project
        .update(cx, |project, cx| {
            project.open_uncommitted_diff(buffer.clone(), cx)
        })
        .await
        .unwrap();
    cx.run_until_parked();

    let mut events_paused = false;
    let mut commit_count = 0;
    for _ in 0..operations {
        match rng.random_range(0..100) {
            0..20 => {
                let snapshot = buffer.read_with(cx, |buffer, _| buffer.text_snapshot());
                let (range, rows) = random_row_range(&snapshot, &mut rng);
                log::info!("staging rows {rows:?}");
                project
                    .update(cx, |project, cx| {
                        project.stage_hunks(buffer.clone(), unstaged_diff.clone(), vec![range], cx)
                    })
                    .unwrap();
            }
            20..35 => {
                let snapshot = buffer.read_with(cx, |buffer, _| buffer.text_snapshot());
                let (range, rows) = random_row_range(&snapshot, &mut rng);
                log::info!("unstaging rows {rows:?} via the uncommitted diff");
                project
                    .update(cx, |project, cx| {
                        project.unstage_uncommitted_hunks(
                            buffer.clone(),
                            uncommitted_diff.clone(),
                            vec![range],
                            cx,
                        )
                    })
                    .unwrap();
            }
            35..50 => {
                let snapshot = index_buffer.read_with(cx, |buffer, _| buffer.text_snapshot());
                let (range, rows) = random_row_range(&snapshot, &mut rng);
                log::info!("unstaging index rows {rows:?} via the staged diff");
                project
                    .update(cx, |project, cx| {
                        project.unstage_staged_hunks(staged_diff.clone(), vec![range], cx)
                    })
                    .unwrap();
            }
            // Line-oriented edits keep the file hunk-rich; uniform random
            // splices (`randomly_edit`) delete a quarter of the file on
            // average and quickly collapse it into a single hunk.
            50..75 => {
                let snapshot = buffer.read_with(cx, |buffer, _| buffer.text_snapshot());
                let max_row = snapshot.max_point().row;
                let row = rng.random_range(0..=max_row);
                let row_start = snapshot.point_to_offset(Point::new(row, 0));
                let row_end = snapshot.point_to_offset(Point::new(row, snapshot.line_len(row)));
                let new_text_len = rng.random_range(0..8);
                let new_text = RandomCharIter::new(&mut rng)
                    .take(new_text_len)
                    .collect::<String>();
                let (range, new_text) = match rng.random_range(0..10) {
                    0..3 => (row_start..row_start, format!("{new_text}\n")),
                    3..5 if max_row > 0 => {
                        let end = if row < max_row {
                            snapshot.point_to_offset(Point::new(row + 1, 0))
                        } else {
                            snapshot.len()
                        };
                        (row_start..end, String::new())
                    }
                    5..8 => (row_start..row_end, new_text),
                    _ => {
                        let start = snapshot
                            .clip_offset(rng.random_range(0..=snapshot.len()), text::Bias::Left);
                        let end = snapshot.clip_offset(
                            (start + rng.random_range(0..10)).min(snapshot.len()),
                            text::Bias::Right,
                        );
                        (start..end, new_text)
                    }
                };
                log::info!("editing buffer: {range:?} -> {new_text:?}");
                buffer.update(cx, |buffer, cx| buffer.edit([(range, new_text)], None, cx));
            }
            // Commits and whole-file round-trip checkpoints. The round trips
            // settle first so the resulting index bytes are exactly
            // predictable, which catches wrong-content writes that the
            // settled-consistency checks can't see (a corrupted write becomes
            // the on-disk truth that everything else then agrees with).
            75..82 => {
                let buffer_full_range = Anchor::min_max_range_for_buffer(
                    buffer.read_with(cx, |buffer, _| buffer.remote_id()),
                );
                let index_full_range = Anchor::min_max_range_for_buffer(
                    index_buffer.read_with(cx, |buffer, _| buffer.remote_id()),
                );
                match rng.random_range(0..4) {
                    // Commit: HEAD becomes whatever the index currently
                    // contains on disk, which may lag in-flight optimistic
                    // writes, just like running `git commit` in a terminal.
                    0 => {
                        let new_head = disk_index_text(&fs);
                        commit_count += 1;
                        log::info!("committing (commit {commit_count})");
                        fs.set_head_for_repo(
                            path!("/dir/.git").as_ref(),
                            &[("file.txt", new_head.clone())],
                            format!("sha-{commit_count}"),
                        );
                        committed_text = new_head;
                    }
                    // Stage everything and commit it, like `git add -A &&
                    // git commit`.
                    1 => {
                        log::info!("checkpoint: stage-all, then commit");
                        if events_paused {
                            fs.unpause_events_and_flush();
                            events_paused = false;
                        }
                        cx.run_until_parked();
                        project
                            .update(cx, |project, cx| {
                                project.stage_hunks(
                                    buffer.clone(),
                                    unstaged_diff.clone(),
                                    vec![buffer_full_range],
                                    cx,
                                )
                            })
                            .unwrap();
                        cx.run_until_parked();
                        let disk = disk_index_text(&fs);
                        let buffer_text = buffer.read_with(cx, |buffer, _| buffer.text());
                        assert_eq!(
                            disk, buffer_text,
                            "after staging everything, the index must equal the worktree"
                        );
                        commit_count += 1;
                        log::info!("committing (commit {commit_count})");
                        fs.set_head_for_repo(
                            path!("/dir/.git").as_ref(),
                            &[("file.txt", disk.clone())],
                            format!("sha-{commit_count}"),
                        );
                        committed_text = disk;
                    }
                    // Unstage everything via the staged diff, which by
                    // definition covers every index-vs-HEAD difference.
                    2 => {
                        log::info!("checkpoint: unstage-all");
                        if events_paused {
                            fs.unpause_events_and_flush();
                            events_paused = false;
                        }
                        cx.run_until_parked();
                        project
                            .update(cx, |project, cx| {
                                project.unstage_staged_hunks(
                                    staged_diff.clone(),
                                    vec![index_full_range],
                                    cx,
                                )
                            })
                            .unwrap();
                        cx.run_until_parked();
                        let disk = disk_index_text(&fs);
                        assert_eq!(
                            disk, committed_text,
                            "after unstaging everything, the index must equal HEAD"
                        );
                    }
                    // Unstage everything and immediately re-stage everything
                    // with no settling in between: the re-stage must supersede
                    // the in-flight unstage (the footprint mechanism), leaving
                    // the index equal to the worktree.
                    _ => {
                        log::info!("checkpoint: unstage-all then stage-all");
                        if events_paused {
                            fs.unpause_events_and_flush();
                            events_paused = false;
                        }
                        cx.run_until_parked();
                        project
                            .update(cx, |project, cx| {
                                project.unstage_staged_hunks(
                                    staged_diff.clone(),
                                    vec![index_full_range],
                                    cx,
                                )
                            })
                            .unwrap();
                        project
                            .update(cx, |project, cx| {
                                project.stage_hunks(
                                    buffer.clone(),
                                    unstaged_diff.clone(),
                                    vec![buffer_full_range],
                                    cx,
                                )
                            })
                            .unwrap();
                        cx.run_until_parked();
                        let disk = disk_index_text(&fs);
                        let buffer_text = buffer.read_with(cx, |buffer, _| buffer.text());
                        assert_eq!(
                            disk, buffer_text,
                            "re-staging everything must supersede the in-flight unstage"
                        );
                    }
                }
            }
            82..90 => {
                if events_paused {
                    log::info!("unpausing fs events");
                    fs.unpause_events_and_flush();
                    events_paused = false;
                } else {
                    log::info!("pausing fs events");
                    fs.pause_events();
                    events_paused = true;
                }
            }
            _ => {
                log::info!("settling");
                if events_paused {
                    fs.unpause_events_and_flush();
                    events_paused = false;
                }
                cx.run_until_parked();
            }
        }

        for _ in 0..rng.random_range(0..5) {
            cx.executor().simulate_random_delay().await;
        }
    }

    if events_paused {
        fs.unpause_events_and_flush();
    }
    cx.run_until_parked();

    assert_settled(&unstaged_diff, "unstaged diff", cx);
    assert_settled(&staged_diff, "staged diff", cx);
    assert_settled(&uncommitted_diff, "uncommitted diff", cx);

    let old_unstaged_state = capture_diff_state(&unstaged_diff, cx);
    let old_staged_state = capture_diff_state(&staged_diff, cx);
    let old_uncommitted_state = capture_diff_state(&uncommitted_diff, cx);

    let disk_index_text = String::from_utf8(
        repo.load_index_text(RepoPath::from_rel_path(rel_path("file.txt")))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        old_unstaged_state.1.as_deref(),
        Some(disk_index_text.as_str()),
        "unstaged diff base text differs from the index on disk"
    );
    assert_eq!(
        old_staged_state.0, disk_index_text,
        "staged diff buffer differs from the index on disk"
    );
    assert_eq!(
        old_staged_state.1.as_deref(),
        Some(committed_text.as_str()),
        "staged diff base text differs from HEAD"
    );
    assert_eq!(
        old_uncommitted_state.1.as_deref(),
        Some(committed_text.as_str()),
        "uncommitted diff base text differs from HEAD"
    );

    // Differential check: a from-scratch computation over (HEAD, index,
    // worktree) must agree with the incrementally-maintained state that just
    // settled.
    let old_entity_ids = [
        unstaged_diff.entity_id(),
        staged_diff.entity_id(),
        uncommitted_diff.entity_id(),
    ];
    drop(unstaged_diff);
    drop(staged_diff);
    drop(uncommitted_diff);
    // An empty update flushes effects so that the dropped entities are
    // actually released before we reopen.
    cx.update(|_| {});
    cx.run_until_parked();

    let unstaged_diff = project
        .update(cx, |project, cx| {
            project.open_unstaged_diff(buffer.clone(), cx)
        })
        .await
        .unwrap();
    let (staged_diff, _) = project
        .update(cx, |project, cx| {
            project.open_staged_diff(buffer.clone(), cx)
        })
        .await
        .unwrap();
    let uncommitted_diff = project
        .update(cx, |project, cx| {
            project.open_uncommitted_diff(buffer.clone(), cx)
        })
        .await
        .unwrap();
    cx.run_until_parked();

    assert_ne!(
        unstaged_diff.entity_id(),
        old_entity_ids[0],
        "the unstaged diff was not actually closed"
    );
    assert_ne!(
        staged_diff.entity_id(),
        old_entity_ids[1],
        "the staged diff was not actually closed"
    );
    assert_ne!(
        uncommitted_diff.entity_id(),
        old_entity_ids[2],
        "the uncommitted diff was not actually closed"
    );

    assert_eq!(
        capture_diff_state(&unstaged_diff, cx),
        old_unstaged_state,
        "reopened unstaged diff differs from the settled one"
    );
    assert_eq!(
        capture_diff_state(&staged_diff, cx),
        old_staged_state,
        "reopened staged diff differs from the settled one"
    );
    assert_eq!(
        capture_diff_state(&uncommitted_diff, cx),
        old_uncommitted_state,
        "reopened uncommitted diff differs from the settled one"
    );
}

#[gpui::test]
async fn test_single_file_diffs(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let committed_contents = r#"
        fn main() {
            println!("hello from HEAD");
        }
    "#
    .unindent();
    let file_contents = r#"
        fn main() {
            println!("hello from the working copy");
        }
    "#
    .unindent();

    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        "/dir",
        json!({
            ".git": {},
           "src": {
               "main.rs": file_contents,
           }
        }),
    )
    .await;

    fs.set_head_for_repo(
        Path::new("/dir/.git"),
        &[("src/main.rs", committed_contents.clone())],
        "deadbeef",
    );
    fs.set_index_for_repo(
        Path::new("/dir/.git"),
        &[("src/main.rs", committed_contents.clone())],
    );

    let project = Project::test(fs.clone(), ["/dir/src/main.rs".as_ref()], cx).await;

    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer("/dir/src/main.rs", cx)
        })
        .await
        .unwrap();
    let uncommitted_diff = project
        .update(cx, |project, cx| {
            project.open_uncommitted_diff(buffer.clone(), cx)
        })
        .await
        .unwrap();

    cx.run_until_parked();
    uncommitted_diff.update(cx, |uncommitted_diff, cx| {
        let snapshot = buffer.read(cx).snapshot();
        assert_hunks(
            uncommitted_diff.snapshot(cx).hunks(&snapshot),
            &snapshot,
            &uncommitted_diff.base_text_string(cx).unwrap(),
            &[(
                1..2,
                "    println!(\"hello from HEAD\");\n",
                "    println!(\"hello from the working copy\");\n",
                DiffHunkStatus {
                    kind: DiffHunkStatusKind::Modified,
                    secondary: DiffHunkSecondaryStatus::HasSecondaryHunk,
                },
            )],
        );
    });
}

// TODO: Should we test this on Windows also?
#[gpui::test]
#[cfg(not(windows))]
async fn test_staging_hunk_preserve_executable_permission(cx: &mut gpui::TestAppContext) {
    use std::os::unix::fs::PermissionsExt;
    init_test(cx);
    cx.executor().allow_parking();
    let committed_contents = "bar\n";
    let file_contents = "baz\n";
    let root = TempTree::new(json!({
        "project": {
            "foo": committed_contents
        },
    }));

    let work_dir = root.path().join("project");
    let file_path = work_dir.join("foo");
    let repo = git_init(work_dir.as_path());
    let mut perms = std::fs::metadata(&file_path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&file_path, perms).unwrap();
    git_add("foo", &repo);
    git_commit("Initial commit", &repo);
    std::fs::write(&file_path, file_contents).unwrap();

    let project = Project::test(
        Arc::new(RealFs::new(None, cx.executor())),
        [root.path()],
        cx,
    )
    .await;

    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer(file_path.as_path(), cx)
        })
        .await
        .unwrap();

    let snapshot = buffer.read_with(cx, |buffer, _| buffer.snapshot());

    let uncommitted_diff = project
        .update(cx, |project, cx| {
            project.open_uncommitted_diff(buffer.clone(), cx)
        })
        .await
        .unwrap();

    let ranges = uncommitted_diff.read_with(cx, |diff, cx| {
        diff.snapshot(cx)
            .hunks(&snapshot)
            .map(|hunk| hunk.buffer_range)
            .collect::<Vec<_>>()
    });
    project
        .update(cx, |project, cx| {
            let unstaged_diff = uncommitted_diff.read(cx).secondary_diff().unwrap();
            project.stage_hunks(buffer.clone(), unstaged_diff, ranges, cx)
        })
        .unwrap();

    cx.run_until_parked();

    let output = smol::process::Command::new("git")
        .current_dir(&work_dir)
        .args(["diff", "--staged"])
        .output()
        .await
        .unwrap();

    let staged_diff = String::from_utf8_lossy(&output.stdout);

    assert!(
        !staged_diff.contains("new mode 100644"),
        "Staging should not change file mode from 755 to 644.\ngit diff --staged:\n{}",
        staged_diff
    );

    let output = smol::process::Command::new("git")
        .current_dir(&work_dir)
        .args(["ls-files", "-s"])
        .output()
        .await
        .unwrap();
    let index_contents = String::from_utf8_lossy(&output.stdout);

    assert!(
        index_contents.contains("100755"),
        "Index should show file as executable (100755).\ngit ls-files -s:\n{}",
        index_contents
    );
}

#[gpui::test]
async fn test_repository_and_path_for_project_path(
    background_executor: BackgroundExecutor,
    cx: &mut gpui::TestAppContext,
) {
    init_test(cx);
    let fs = FakeFs::new(background_executor);
    fs.insert_tree(
        path!("/root"),
        json!({
            "c.txt": "",
            "dir1": {
                ".git": {},
                "deps": {
                    "dep1": {
                        ".git": {},
                        "src": {
                            "a.txt": ""
                        }
                    }
                },
                "src": {
                    "b.txt": ""
                }
            },
        }),
    )
    .await;

    let project = Project::test(fs.clone(), [path!("/root").as_ref()], cx).await;
    let tree = project.read_with(cx, |project, cx| project.worktrees(cx).next().unwrap());
    let tree_id = tree.read_with(cx, |tree, _| tree.id());
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.run_until_parked();

    project.read_with(cx, |project, cx| {
        let git_store = project.git_store().read(cx);
        let pairs = [
            ("c.txt", None),
            ("dir1/src/b.txt", Some((path!("/root/dir1"), "src/b.txt"))),
            (
                "dir1/deps/dep1/src/a.txt",
                Some((path!("/root/dir1/deps/dep1"), "src/a.txt")),
            ),
        ];
        let expected = pairs
            .iter()
            .map(|(path, result)| {
                (
                    path,
                    result.map(|(repo, repo_path)| {
                        (Path::new(repo).into(), RepoPath::new(repo_path).unwrap())
                    }),
                )
            })
            .collect::<Vec<_>>();
        let actual = pairs
            .iter()
            .map(|(path, _)| {
                let project_path = (tree_id, rel_path(path)).into();
                let result = maybe!({
                    let (repo, repo_path) =
                        git_store.repository_and_path_for_project_path(&project_path, cx)?;
                    Some((repo.read(cx).work_directory_abs_path.clone(), repo_path))
                });
                (path, result)
            })
            .collect::<Vec<_>>();
        pretty_assertions::assert_eq!(expected, actual);
    });

    fs.remove_dir(path!("/root/dir1/.git").as_ref(), RemoveOptions::default())
        .await
        .unwrap();
    cx.run_until_parked();

    project.read_with(cx, |project, cx| {
        let git_store = project.git_store().read(cx);
        assert_eq!(
            git_store.repository_and_path_for_project_path(
                &(tree_id, rel_path("dir1/src/b.txt")).into(),
                cx
            ),
            None
        );
    });
}

#[gpui::test]
async fn test_home_dir_as_git_repository(cx: &mut gpui::TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.background_executor.clone());
    let home = paths::home_dir();
    fs.insert_tree(
        home,
        json!({
            ".git": {},
            "project": {
                "a.txt": "A"
            },
        }),
    )
    .await;

    let project = Project::test(fs.clone(), [home.join("project").as_ref()], cx).await;
    let tree = project.read_with(cx, |project, cx| project.worktrees(cx).next().unwrap());
    let tree_id = tree.read_with(cx, |tree, _| tree.id());

    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    tree.flush_fs_events(cx).await;

    project.read_with(cx, |project, cx| {
        let containing = project
            .git_store()
            .read(cx)
            .repository_and_path_for_project_path(&(tree_id, rel_path("a.txt")).into(), cx);
        assert!(containing.is_none());
    });

    let project = Project::test(fs.clone(), [home.as_ref()], cx).await;
    let tree = project.read_with(cx, |project, cx| project.worktrees(cx).next().unwrap());
    let tree_id = tree.read_with(cx, |tree, _| tree.id());
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    tree.flush_fs_events(cx).await;

    project.read_with(cx, |project, cx| {
        let containing = project
            .git_store()
            .read(cx)
            .repository_and_path_for_project_path(&(tree_id, rel_path("project/a.txt")).into(), cx);
        assert_eq!(
            containing
                .unwrap()
                .0
                .read(cx)
                .work_directory_abs_path
                .as_ref(),
            home,
        );
    });
}

#[gpui::test]
async fn test_git_repository_status(cx: &mut gpui::TestAppContext) {
    init_test(cx);
    cx.executor().allow_parking();

    let root = TempTree::new(json!({
        "project": {
            "a.txt": "a",    // Modified
            "b.txt": "bb",   // Added
            "c.txt": "ccc",  // Unchanged
            "d.txt": "dddd", // Deleted
        },
    }));

    // Set up git repository before creating the project.
    let work_dir = root.path().join("project");
    let repo = git_init(work_dir.as_path());
    git_add("a.txt", &repo);
    git_add("c.txt", &repo);
    git_add("d.txt", &repo);
    git_commit("Initial commit", &repo);
    std::fs::remove_file(work_dir.join("d.txt")).unwrap();
    std::fs::write(work_dir.join("a.txt"), "aa").unwrap();

    let project = Project::test(
        Arc::new(RealFs::new(None, cx.executor())),
        [root.path()],
        cx,
    )
    .await;

    let tree = project.read_with(cx, |project, cx| project.worktrees(cx).next().unwrap());
    tree.flush_fs_events(cx).await;
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.executor().run_until_parked();

    let repository = project.read_with(cx, |project, cx| {
        project.repositories(cx).values().next().unwrap().clone()
    });

    // Check that the right git state is observed on startup
    repository.read_with(cx, |repository, _| {
        let entries = repository.cached_status().collect::<Vec<_>>();
        assert_eq!(
            entries,
            [
                StatusEntry {
                    repo_path: repo_path("a.txt"),
                    status: StatusCode::Modified.worktree(),
                    diff_stat: Some(DiffStat {
                        added: 1,
                        deleted: 1,
                    }),
                    staged_diff_stat: None,
                    unstaged_diff_stat: Some(DiffStat {
                        added: 1,
                        deleted: 1,
                    }),
                },
                StatusEntry {
                    repo_path: repo_path("b.txt"),
                    status: FileStatus::Untracked,
                    diff_stat: None,
                    staged_diff_stat: None,
                    unstaged_diff_stat: None,
                },
                StatusEntry {
                    repo_path: repo_path("d.txt"),
                    status: StatusCode::Deleted.worktree(),
                    diff_stat: Some(DiffStat {
                        added: 0,
                        deleted: 1,
                    }),
                    staged_diff_stat: None,
                    unstaged_diff_stat: Some(DiffStat {
                        added: 0,
                        deleted: 1,
                    }),
                },
            ]
        );
    });

    std::fs::write(work_dir.join("c.txt"), "some changes").unwrap();

    tree.flush_fs_events(cx).await;
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.executor().run_until_parked();

    repository.read_with(cx, |repository, _| {
        let entries = repository.cached_status().collect::<Vec<_>>();
        assert_eq!(
            entries,
            [
                StatusEntry {
                    repo_path: repo_path("a.txt"),
                    status: StatusCode::Modified.worktree(),
                    diff_stat: Some(DiffStat {
                        added: 1,
                        deleted: 1,
                    }),
                    staged_diff_stat: None,
                    unstaged_diff_stat: Some(DiffStat {
                        added: 1,
                        deleted: 1,
                    }),
                },
                StatusEntry {
                    repo_path: repo_path("b.txt"),
                    status: FileStatus::Untracked,
                    diff_stat: None,
                    staged_diff_stat: None,
                    unstaged_diff_stat: None,
                },
                StatusEntry {
                    repo_path: repo_path("c.txt"),
                    status: StatusCode::Modified.worktree(),
                    diff_stat: Some(DiffStat {
                        added: 1,
                        deleted: 1,
                    }),
                    staged_diff_stat: None,
                    unstaged_diff_stat: Some(DiffStat {
                        added: 1,
                        deleted: 1,
                    }),
                },
                StatusEntry {
                    repo_path: repo_path("d.txt"),
                    status: StatusCode::Deleted.worktree(),
                    diff_stat: Some(DiffStat {
                        added: 0,
                        deleted: 1,
                    }),
                    staged_diff_stat: None,
                    unstaged_diff_stat: Some(DiffStat {
                        added: 0,
                        deleted: 1,
                    }),
                },
            ]
        );
    });

    git_add("a.txt", &repo);
    git_add("c.txt", &repo);
    git_remove_index(Path::new("d.txt"), &repo);
    git_commit("Another commit", &repo);
    tree.flush_fs_events(cx).await;
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.executor().run_until_parked();

    std::fs::remove_file(work_dir.join("a.txt")).unwrap();
    std::fs::remove_file(work_dir.join("b.txt")).unwrap();
    tree.flush_fs_events(cx).await;
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.executor().run_until_parked();

    repository.read_with(cx, |repository, _cx| {
        let entries = repository.cached_status().collect::<Vec<_>>();

        // Deleting an untracked entry, b.txt, should leave no status
        // a.txt was tracked, and so should have a status
        assert_eq!(
            entries,
            [StatusEntry {
                repo_path: repo_path("a.txt"),
                status: StatusCode::Deleted.worktree(),
                diff_stat: Some(DiffStat {
                    added: 0,
                    deleted: 1,
                }),
                staged_diff_stat: None,
                unstaged_diff_stat: Some(DiffStat {
                    added: 0,
                    deleted: 1,
                }),
            }]
        );
    });
}

#[gpui::test]
async fn test_git_repository_status_removes_directory_descendants(cx: &mut gpui::TestAppContext) {
    init_test(cx);
    cx.executor().allow_parking();

    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        path!("/root"),
        json!({
            "project": {
                ".git": {},
                "ci2": {
                    "Dockerfile.namespace": "untracked",
                },
            },
        }),
    )
    .await;
    fs.set_status_for_repo(
        path!("/root/project/.git").as_ref(),
        &[("ci2/Dockerfile.namespace", FileStatus::Untracked)],
    );

    let project = Project::test(fs.clone(), [path!("/root/project").as_ref()], cx).await;

    let tree = project.read_with(cx, |project, cx| project.worktrees(cx).next().unwrap());
    tree.flush_fs_events(cx).await;
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.executor().run_until_parked();

    let repository = project.read_with(cx, |project, cx| {
        project.repositories(cx).values().next().unwrap().clone()
    });

    repository.read_with(cx, |repository, _| {
        assert_eq!(
            repository.cached_status().collect::<Vec<_>>(),
            [StatusEntry {
                repo_path: repo_path("ci2/Dockerfile.namespace"),
                status: FileStatus::Untracked,
                diff_stat: None,
                staged_diff_stat: None,
                unstaged_diff_stat: None,
            }]
        );
    });

    fs.pause_events();
    fs.create_dir(path!("/root/project/ci3").as_ref())
        .await
        .unwrap();
    fs.copy_file(
        path!("/root/project/ci2/Dockerfile.namespace").as_ref(),
        path!("/root/project/ci3/Dockerfile.namespace").as_ref(),
        Default::default(),
    )
    .await
    .unwrap();
    fs.remove_dir(
        path!("/root/project/ci2").as_ref(),
        RemoveOptions {
            recursive: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    fs.clear_buffered_events();
    fs.unpause_events_and_flush();
    fs.emit_fs_event(path!("/root/project/ci2"), Some(PathEventKind::Removed));
    fs.emit_fs_event(path!("/root/project/ci3"), Some(PathEventKind::Created));

    tree.flush_fs_events(cx).await;
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.executor().run_until_parked();

    repository.read_with(cx, |repository, _| {
        assert_eq!(
            repository.cached_status().collect::<Vec<_>>(),
            [StatusEntry {
                repo_path: repo_path("ci3/Dockerfile.namespace"),
                status: FileStatus::Untracked,
                diff_stat: None,
                staged_diff_stat: None,
                unstaged_diff_stat: None,
            }]
        );
    });
}

#[cfg(target_os = "linux")]
#[gpui::test(retries = 5)]
async fn test_git_events_after_project_excludes_dot_git(cx: &mut gpui::TestAppContext) {
    init_test(cx);
    cx.executor().allow_parking();

    cx.update(|cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |settings| {
                settings.project.worktree.file_scan_exclusions =
                    Some(SplicingVec::from(vec!["foo".to_string()]));
            });
        });
    });

    let root = TempTree::new(json!({
        "project": {
            "a.txt": "a",
        },
    }));

    let work_dir = root.path().join("project");
    let repo = git_init(&work_dir);
    git_add("a.txt", &repo);
    git_commit("Initial commit", &repo);
    git_branch("other-branch", &repo);

    let project = Project::test(
        Arc::new(RealFs::new(None, cx.executor())),
        [work_dir.as_path()],
        cx,
    )
    .await;

    let tree = project.read_with(cx, |project, cx| project.worktrees(cx).next().unwrap());
    tree.flush_fs_events(cx).await;
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.executor().run_until_parked();

    let repository = project.read_with(cx, |project, cx| {
        project.repositories(cx).values().next().unwrap().clone()
    });
    let branch = repository.read_with(cx, |repository, _| {
        repository
            .snapshot()
            .branch
            .as_ref()
            .map(|branch| branch.ref_name.to_string())
    });
    assert_eq!(branch.as_deref(), Some("refs/heads/main"));

    let worktree_id = tree.read_with(cx, |tree, _| tree.id());
    cx.update_global::<SettingsStore, _>(|store, cx| {
        store
            .set_local_settings(
                worktree_id,
                LocalSettingsPath::InWorktree(Arc::from(RelPath::empty())),
                LocalSettingsKind::Settings,
                Some(r#"{ "file_scan_exclusions": ["**/.git"] }"#),
                cx,
            )
            .unwrap();
    });
    cx.read(|cx| tree.read(cx).as_local().unwrap().scan_complete())
        .await;
    cx.executor().run_until_parked();

    cx.update(|cx| {
        assert!(tree.read(cx).entry_for_path(rel_path(".git")).is_none());
    });

    git_checkout("other-branch", &repo);

    let mut events = cx.events::<RepositoryEvent, _>(&repository);
    let timeout = futures::FutureExt::fuse(cx.background_executor.timer(Duration::from_secs(5)));
    futures::pin_mut!(timeout);
    loop {
        let branch = repository.read_with(cx, |repository, _| {
            repository
                .snapshot()
                .branch
                .as_ref()
                .map(|branch| branch.ref_name.to_string())
        });
        if branch.as_deref() == Some("refs/heads/other-branch") {
            break;
        }

        futures::select_biased! {
            _ = events.next() => {}
            _ = timeout => panic!("timed out waiting for repository HEAD update after .git was excluded"),
        }
    }
}

#[gpui::test]
#[ignore]
async fn test_git_status_postprocessing(cx: &mut gpui::TestAppContext) {
    init_test(cx);
    cx.executor().allow_parking();

    let root = TempTree::new(json!({
        "project": {
            "sub": {},
            "a.txt": "",
        },
    }));

    let work_dir = root.path().join("project");
    let repo = git_init(work_dir.as_path());
    // a.txt exists in HEAD and the working copy but is deleted in the index.
    git_add("a.txt", &repo);
    git_commit("Initial commit", &repo);
    git_remove_index("a.txt".as_ref(), &repo);
    // `sub` is a nested git repository.
    let _sub = git_init(&work_dir.join("sub"));

    let project = Project::test(
        Arc::new(RealFs::new(None, cx.executor())),
        [root.path()],
        cx,
    )
    .await;

    let tree = project.read_with(cx, |project, cx| project.worktrees(cx).next().unwrap());
    tree.flush_fs_events(cx).await;
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.executor().run_until_parked();

    let repository = project.read_with(cx, |project, cx| {
        project
            .repositories(cx)
            .values()
            .find(|repo| repo.read(cx).work_directory_abs_path.ends_with("project"))
            .unwrap()
            .clone()
    });

    repository.read_with(cx, |repository, _cx| {
        let entries = repository.cached_status().collect::<Vec<_>>();

        // `sub` doesn't appear in our computed statuses.
        // a.txt appears with a combined `DA` status.
        assert_eq!(
            entries,
            [StatusEntry {
                repo_path: repo_path("a.txt"),
                status: TrackedStatus {
                    index_status: StatusCode::Deleted,
                    worktree_status: StatusCode::Added
                }
                .into(),
                diff_stat: None,
                staged_diff_stat: Some(DiffStat {
                    added: 0,
                    deleted: 0,
                }),
                unstaged_diff_stat: None,
            }]
        )
    });
}

#[track_caller]
/// We merge lhs into rhs.
fn merge_pending_ops_snapshots(
    source: Vec<pending_op::PendingOps>,
    mut target: Vec<pending_op::PendingOps>,
) -> Vec<pending_op::PendingOps> {
    for s_ops in source {
        if let Some(idx) = target.iter().zip(0..).find_map(|(ops, idx)| {
            if ops.repo_path == s_ops.repo_path {
                Some(idx)
            } else {
                None
            }
        }) {
            let t_ops = &mut target[idx];
            for s_op in s_ops.ops {
                if let Some(op_idx) = t_ops
                    .ops
                    .iter()
                    .zip(0..)
                    .find_map(|(op, idx)| if op.id == s_op.id { Some(idx) } else { None })
                {
                    let t_op = &mut t_ops.ops[op_idx];
                    match (s_op.job_status, t_op.job_status) {
                        (pending_op::JobStatus::Running, _) => {}
                        (s_st, pending_op::JobStatus::Running) => t_op.job_status = s_st,
                        (s_st, t_st) if s_st == t_st => {}
                        _ => unreachable!(),
                    }
                } else {
                    t_ops.ops.push(s_op);
                }
            }
            t_ops.ops.sort_by_key(|op| op.id);
        } else {
            target.push(s_ops);
        }
    }
    target
}

#[gpui::test]
async fn test_repository_pending_ops_staging(
    executor: gpui::BackgroundExecutor,
    cx: &mut gpui::TestAppContext,
) {
    init_test(cx);

    let fs = FakeFs::new(executor);
    fs.insert_tree(
        path!("/root"),
        json!({
            "my-repo": {
                ".git": {},
                "a.txt": "a",
            }

        }),
    )
    .await;

    fs.set_status_for_repo(
        path!("/root/my-repo/.git").as_ref(),
        &[("a.txt", FileStatus::Untracked)],
    );

    let project = Project::test(fs.clone(), [path!("/root/my-repo").as_ref()], cx).await;
    let pending_ops_all = Arc::new(Mutex::new(SumTree::default()));
    project.update(cx, |project, cx| {
        let pending_ops_all = pending_ops_all.clone();
        cx.subscribe(project.git_store(), move |_, _, e, _| {
            if let GitStoreEvent::RepositoryUpdated(
                _,
                RepositoryEvent::PendingOpsChanged { pending_ops },
                _,
            ) = e
            {
                let merged = merge_pending_ops_snapshots(
                    pending_ops.items(()),
                    pending_ops_all.lock().items(()),
                );
                *pending_ops_all.lock() = SumTree::from_iter(merged.into_iter(), ());
            }
        })
        .detach();
    });
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;

    let repo = project.read_with(cx, |project, cx| {
        project.repositories(cx).values().next().unwrap().clone()
    });

    // Ensure we have no pending ops for any of the untracked files
    repo.read_with(cx, |repo, _cx| {
        assert!(repo.pending_ops().next().is_none());
    });

    let mut id = 1u16;

    let mut assert_stage = async |path: RepoPath, stage| {
        let git_status = if stage {
            pending_op::GitStatus::Staged
        } else {
            pending_op::GitStatus::Unstaged
        };
        repo.update(cx, |repo, cx| {
            let task = if stage {
                repo.stage_entries(vec![path.clone()], cx)
            } else {
                repo.unstage_entries(vec![path.clone()], cx)
            };
            let ops = repo.pending_ops_for_path(&path).unwrap();
            assert_eq!(
                ops.ops.last(),
                Some(&pending_op::PendingOp {
                    id: id.into(),
                    git_status,
                    job_status: pending_op::JobStatus::Running
                })
            );
            task
        })
        .await
        .unwrap();

        repo.read_with(cx, |repo, _cx| {
            let ops = repo.pending_ops_for_path(&path).unwrap();
            assert_eq!(
                ops.ops.last(),
                Some(&pending_op::PendingOp {
                    id: id.into(),
                    git_status,
                    job_status: pending_op::JobStatus::Finished
                })
            );
        });

        id += 1;
    };

    assert_stage(repo_path("a.txt"), true).await;
    assert_stage(repo_path("a.txt"), false).await;
    assert_stage(repo_path("a.txt"), true).await;
    assert_stage(repo_path("a.txt"), false).await;
    assert_stage(repo_path("a.txt"), true).await;

    cx.run_until_parked();

    assert_eq!(
        pending_ops_all
            .lock()
            .get(&worktree::PathKey(repo_path("a.txt").as_ref().clone()), ())
            .unwrap()
            .ops,
        vec![
            pending_op::PendingOp {
                id: 1u16.into(),
                git_status: pending_op::GitStatus::Staged,
                job_status: pending_op::JobStatus::Finished
            },
            pending_op::PendingOp {
                id: 2u16.into(),
                git_status: pending_op::GitStatus::Unstaged,
                job_status: pending_op::JobStatus::Finished
            },
            pending_op::PendingOp {
                id: 3u16.into(),
                git_status: pending_op::GitStatus::Staged,
                job_status: pending_op::JobStatus::Finished
            },
            pending_op::PendingOp {
                id: 4u16.into(),
                git_status: pending_op::GitStatus::Unstaged,
                job_status: pending_op::JobStatus::Finished
            },
            pending_op::PendingOp {
                id: 5u16.into(),
                git_status: pending_op::GitStatus::Staged,
                job_status: pending_op::JobStatus::Finished
            }
        ],
    );

    repo.update(cx, |repo, _cx| {
        let git_statuses = repo.cached_status().collect::<Vec<_>>();

        assert_eq!(
            git_statuses,
            [StatusEntry {
                repo_path: repo_path("a.txt"),
                status: TrackedStatus {
                    index_status: StatusCode::Added,
                    worktree_status: StatusCode::Unmodified
                }
                .into(),
                diff_stat: Some(DiffStat {
                    added: 1,
                    deleted: 0,
                }),
                staged_diff_stat: Some(DiffStat {
                    added: 1,
                    deleted: 0,
                }),
                unstaged_diff_stat: None,
            }]
        );
    });
}

#[gpui::test]
async fn test_repository_pending_ops_long_running_staging(
    executor: gpui::BackgroundExecutor,
    cx: &mut gpui::TestAppContext,
) {
    init_test(cx);

    let fs = FakeFs::new(executor);
    fs.insert_tree(
        path!("/root"),
        json!({
            "my-repo": {
                ".git": {},
                "a.txt": "a",
            }

        }),
    )
    .await;

    fs.set_status_for_repo(
        path!("/root/my-repo/.git").as_ref(),
        &[("a.txt", FileStatus::Untracked)],
    );

    let project = Project::test(fs.clone(), [path!("/root/my-repo").as_ref()], cx).await;
    let pending_ops_all = Arc::new(Mutex::new(SumTree::default()));
    project.update(cx, |project, cx| {
        let pending_ops_all = pending_ops_all.clone();
        cx.subscribe(project.git_store(), move |_, _, e, _| {
            if let GitStoreEvent::RepositoryUpdated(
                _,
                RepositoryEvent::PendingOpsChanged { pending_ops },
                _,
            ) = e
            {
                let merged = merge_pending_ops_snapshots(
                    pending_ops.items(()),
                    pending_ops_all.lock().items(()),
                );
                *pending_ops_all.lock() = SumTree::from_iter(merged.into_iter(), ());
            }
        })
        .detach();
    });

    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;

    let repo = project.read_with(cx, |project, cx| {
        project.repositories(cx).values().next().unwrap().clone()
    });

    repo.update(cx, |repo, cx| {
        repo.stage_entries(vec![repo_path("a.txt")], cx)
    })
    .detach();

    repo.update(cx, |repo, cx| {
        repo.stage_entries(vec![repo_path("a.txt")], cx)
    })
    .unwrap()
    .with_timeout(Duration::from_secs(1), &cx.executor())
    .await
    .unwrap();

    cx.run_until_parked();

    assert_eq!(
        pending_ops_all
            .lock()
            .get(&worktree::PathKey(repo_path("a.txt").as_ref().clone()), ())
            .unwrap()
            .ops,
        vec![
            pending_op::PendingOp {
                id: 1u16.into(),
                git_status: pending_op::GitStatus::Staged,
                job_status: pending_op::JobStatus::Skipped
            },
            pending_op::PendingOp {
                id: 2u16.into(),
                git_status: pending_op::GitStatus::Staged,
                job_status: pending_op::JobStatus::Finished
            }
        ],
    );

    repo.update(cx, |repo, _cx| {
        let git_statuses = repo.cached_status().collect::<Vec<_>>();

        assert_eq!(
            git_statuses,
            [StatusEntry {
                repo_path: repo_path("a.txt"),
                status: TrackedStatus {
                    index_status: StatusCode::Added,
                    worktree_status: StatusCode::Unmodified
                }
                .into(),
                diff_stat: Some(DiffStat {
                    added: 1,
                    deleted: 0,
                }),
                staged_diff_stat: Some(DiffStat {
                    added: 1,
                    deleted: 0,
                }),
                unstaged_diff_stat: None,
            }]
        );
    });
}

#[gpui::test]
async fn test_repository_pending_ops_stage_all(
    executor: gpui::BackgroundExecutor,
    cx: &mut gpui::TestAppContext,
) {
    init_test(cx);

    let fs = FakeFs::new(executor);
    fs.insert_tree(
        path!("/root"),
        json!({
            "my-repo": {
                ".git": {},
                "a.txt": "a",
                "b.txt": "b"
            }

        }),
    )
    .await;

    fs.set_status_for_repo(
        path!("/root/my-repo/.git").as_ref(),
        &[
            ("a.txt", FileStatus::Untracked),
            ("b.txt", FileStatus::Untracked),
        ],
    );

    let project = Project::test(fs.clone(), [path!("/root/my-repo").as_ref()], cx).await;
    let pending_ops_all = Arc::new(Mutex::new(SumTree::default()));
    project.update(cx, |project, cx| {
        let pending_ops_all = pending_ops_all.clone();
        cx.subscribe(project.git_store(), move |_, _, e, _| {
            if let GitStoreEvent::RepositoryUpdated(
                _,
                RepositoryEvent::PendingOpsChanged { pending_ops },
                _,
            ) = e
            {
                let merged = merge_pending_ops_snapshots(
                    pending_ops.items(()),
                    pending_ops_all.lock().items(()),
                );
                *pending_ops_all.lock() = SumTree::from_iter(merged.into_iter(), ());
            }
        })
        .detach();
    });
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;

    let repo = project.read_with(cx, |project, cx| {
        project.repositories(cx).values().next().unwrap().clone()
    });

    repo.update(cx, |repo, cx| {
        repo.stage_entries(vec![repo_path("a.txt")], cx)
    })
    .await
    .unwrap();
    repo.update(cx, |repo, cx| repo.stage_all(cx))
        .await
        .unwrap();
    repo.update(cx, |repo, cx| repo.unstage_all(cx))
        .await
        .unwrap();

    cx.run_until_parked();

    assert_eq!(
        pending_ops_all
            .lock()
            .get(&worktree::PathKey(repo_path("a.txt").as_ref().clone()), ())
            .unwrap()
            .ops,
        vec![
            pending_op::PendingOp {
                id: 1u16.into(),
                git_status: pending_op::GitStatus::Staged,
                job_status: pending_op::JobStatus::Finished
            },
            pending_op::PendingOp {
                id: 2u16.into(),
                git_status: pending_op::GitStatus::Unstaged,
                job_status: pending_op::JobStatus::Finished
            },
        ],
    );
    assert_eq!(
        pending_ops_all
            .lock()
            .get(&worktree::PathKey(repo_path("b.txt").as_ref().clone()), ())
            .unwrap()
            .ops,
        vec![
            pending_op::PendingOp {
                id: 1u16.into(),
                git_status: pending_op::GitStatus::Staged,
                job_status: pending_op::JobStatus::Finished
            },
            pending_op::PendingOp {
                id: 2u16.into(),
                git_status: pending_op::GitStatus::Unstaged,
                job_status: pending_op::JobStatus::Finished
            },
        ],
    );

    repo.update(cx, |repo, _cx| {
        let git_statuses = repo.cached_status().collect::<Vec<_>>();

        assert_eq!(
            git_statuses,
            [
                StatusEntry {
                    repo_path: repo_path("a.txt"),
                    status: FileStatus::Untracked,
                    diff_stat: None,
                    staged_diff_stat: None,
                    unstaged_diff_stat: None,
                },
                StatusEntry {
                    repo_path: repo_path("b.txt"),
                    status: FileStatus::Untracked,
                    diff_stat: None,
                    staged_diff_stat: None,
                    unstaged_diff_stat: None,
                },
            ]
        );
    });
}

#[gpui::test]
async fn test_project_group_keys_remain_distinct_for_sibling_repo_subdirectories(
    executor: gpui::BackgroundExecutor,
    cx: &mut gpui::TestAppContext,
) {
    init_test(cx);

    let fs = FakeFs::new(executor);
    fs.insert_tree(
        path!("/root"),
        json!({
            "my-repo": {
                ".git": {},
                "packages": {
                    "a": { "file.txt": "a" },
                    "b": { "file.txt": "b" },
                },
            },
        }),
    )
    .await;

    let project_a =
        Project::test(fs.clone(), [path!("/root/my-repo/packages/a").as_ref()], cx).await;
    let project_b = Project::test(fs, [path!("/root/my-repo/packages/b").as_ref()], cx).await;

    project_a
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    project_b
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.run_until_parked();

    let key_a = project_a.read_with(cx, |project, cx| ProjectGroupKey::from_project(project, cx));
    let key_b = project_b.read_with(cx, |project, cx| ProjectGroupKey::from_project(project, cx));

    assert_ne!(key_a, key_b);
    assert_eq!(
        key_a
            .path_list()
            .ordered_paths()
            .map(|path| path.as_path())
            .collect::<Vec<_>>(),
        vec![Path::new(path!("/root/my-repo/packages/a"))]
    );
    assert_eq!(
        key_b
            .path_list()
            .ordered_paths()
            .map(|path| path.as_path())
            .collect::<Vec<_>>(),
        vec![Path::new(path!("/root/my-repo/packages/b"))]
    );
}

fn project_group_key_paths(project: &Entity<Project>, cx: &TestAppContext) -> Vec<PathBuf> {
    project.read_with(cx, |project, cx| {
        ProjectGroupKey::from_project(project, cx)
            .path_list()
            .ordered_paths()
            .cloned()
            .collect()
    })
}

fn project_worktree_paths(
    project: &Entity<Project>,
    cx: &TestAppContext,
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    project.read_with(cx, |project, cx| {
        let paths = project.worktree_paths(cx);
        (
            paths.folder_path_list().ordered_paths().cloned().collect(),
            paths
                .main_worktree_path_list()
                .ordered_paths()
                .cloned()
                .collect(),
        )
    })
}

#[gpui::test]
async fn test_project_group_keys_match_for_bare_repo_linked_worktrees(
    executor: gpui::BackgroundExecutor,
    cx: &mut gpui::TestAppContext,
) {
    init_test(cx);

    let fs = FakeFs::new(executor);
    fs.insert_tree(
        path!("/root"),
        json!({
            "monty": {
                ".git": "gitdir: ./.bare\n",
                ".bare": {
                    "HEAD": "ref: refs/heads/main\n",
                    "worktrees": {
                        "main": {
                            "commondir": "../..",
                            "gitdir": "/root/monty/main/.git",
                        },
                        "feature-a": {
                            "commondir": "../..",
                            "gitdir": "/root/monty/feature-a/.git",
                        },
                        "feature-b": {
                            "commondir": "../..",
                            "gitdir": "/root/monty/feature-b/.git",
                        },
                    },
                },
                "main": {
                    ".git": "gitdir: /root/monty/.bare/worktrees/main\n",
                    "file.txt": "main",
                },
                "feature-a": {
                    ".git": "gitdir: /root/monty/.bare/worktrees/feature-a\n",
                    "file.txt": "a",
                },
                "feature-b": {
                    ".git": "gitdir: /root/monty/.bare/worktrees/feature-b\n",
                    "file.txt": "b",
                },
            },
        }),
    )
    .await;

    let project_root = Project::test(fs.clone(), [path!("/root/monty").as_ref()], cx).await;
    let project_main = Project::test(fs.clone(), [path!("/root/monty/main").as_ref()], cx).await;
    let project_a = Project::test(fs.clone(), [path!("/root/monty/feature-a").as_ref()], cx).await;
    let project_b = Project::test(fs, [path!("/root/monty/feature-b").as_ref()], cx).await;

    project_root
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    project_main
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    project_a
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    project_b
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.run_until_parked();

    for project in [&project_root, &project_main, &project_a, &project_b] {
        assert_eq!(
            project_group_key_paths(project, cx),
            vec![PathBuf::from(path!("/root/monty"))]
        );
    }

    assert_eq!(
        project_worktree_paths(&project_root, cx),
        (
            vec![PathBuf::from(path!("/root/monty"))],
            vec![PathBuf::from(path!("/root/monty"))],
        )
    );
    assert_eq!(
        project_worktree_paths(&project_main, cx),
        (
            vec![PathBuf::from(path!("/root/monty/main"))],
            vec![PathBuf::from(path!("/root/monty"))],
        )
    );
    assert_eq!(
        project_worktree_paths(&project_a, cx),
        (
            vec![PathBuf::from(path!("/root/monty/feature-a"))],
            vec![PathBuf::from(path!("/root/monty"))],
        )
    );
    assert_eq!(
        project_worktree_paths(&project_b, cx),
        (
            vec![PathBuf::from(path!("/root/monty/feature-b"))],
            vec![PathBuf::from(path!("/root/monty"))],
        )
    );
}

#[gpui::test]
async fn test_project_group_key_groups_nested_linked_worktree_under_main_repo(
    executor: gpui::BackgroundExecutor,
    cx: &mut gpui::TestAppContext,
) {
    init_test(cx);

    let fs = FakeFs::new(executor);
    fs.insert_tree(
        path!("/root"),
        json!({
            "my-repo": {
                ".git": {},
                "file.txt": "content",
            },
        }),
    )
    .await;

    let linked_worktree_path = PathBuf::from(path!("/root/my-repo/.zed/worktrees/feature"));
    fs.add_linked_worktree_for_repo(
        Path::new(path!("/root/my-repo/.git")),
        false,
        git::repository::Worktree {
            path: linked_worktree_path.clone(),
            ref_name: Some("refs/heads/feature".into()),
            sha: "abc123".into(),
            is_main: false,
            is_bare: false,
        },
    )
    .await;

    let project = Project::test(fs, [linked_worktree_path.as_path()], cx).await;
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.run_until_parked();

    assert_eq!(
        project_group_key_paths(&project, cx),
        vec![PathBuf::from(path!("/root/my-repo"))]
    );
    assert_eq!(
        project_worktree_paths(&project, cx),
        (
            vec![PathBuf::from(path!("/root/my-repo/.zed/worktrees/feature"))],
            vec![PathBuf::from(path!("/root/my-repo"))],
        )
    );
}

#[gpui::test]
async fn test_repository_subfolder_git_status(
    executor: gpui::BackgroundExecutor,
    cx: &mut gpui::TestAppContext,
) {
    init_test(cx);

    let fs = FakeFs::new(executor);
    fs.insert_tree(
        path!("/root"),
        json!({
            "my-repo": {
                ".git": {},
                "a.txt": "a",
                "sub-folder-1": {
                    "sub-folder-2": {
                        "c.txt": "cc",
                        "d": {
                            "e.txt": "eee"
                        }
                    },
                }
            },
        }),
    )
    .await;

    const C_TXT: &str = "sub-folder-1/sub-folder-2/c.txt";
    const E_TXT: &str = "sub-folder-1/sub-folder-2/d/e.txt";

    fs.set_status_for_repo(
        path!("/root/my-repo/.git").as_ref(),
        &[(E_TXT, FileStatus::Untracked)],
    );

    let project = Project::test(
        fs.clone(),
        [path!("/root/my-repo/sub-folder-1/sub-folder-2").as_ref()],
        cx,
    )
    .await;

    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.run_until_parked();

    let repository = project.read_with(cx, |project, cx| {
        project.repositories(cx).values().next().unwrap().clone()
    });

    let worktree_paths = project.read_with(cx, |project, cx| project.worktree_paths(cx));
    assert_eq!(
        worktree_paths
            .main_worktree_path_list()
            .ordered_paths()
            .map(|path| path.as_path())
            .collect::<Vec<_>>(),
        vec![Path::new(path!("/root/my-repo/sub-folder-1/sub-folder-2"))]
    );

    // Ensure that the git status is loaded correctly
    repository.read_with(cx, |repository, _cx| {
        assert_eq!(
            repository.work_directory_abs_path,
            Path::new(path!("/root/my-repo")).into()
        );

        assert_eq!(repository.status_for_path(&repo_path(C_TXT)), None);
        assert_eq!(
            repository
                .status_for_path(&repo_path(E_TXT))
                .unwrap()
                .status,
            FileStatus::Untracked
        );
    });

    fs.set_status_for_repo(path!("/root/my-repo/.git").as_ref(), &[]);
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.run_until_parked();

    repository.read_with(cx, |repository, _cx| {
        assert_eq!(repository.status_for_path(&repo_path(C_TXT)), None);
        assert_eq!(repository.status_for_path(&repo_path(E_TXT)), None);
    });
}

// TODO: this test is flaky (especially on Windows but at least sometimes on all platforms).
#[cfg(any())]
#[gpui::test]
async fn test_conflicted_cherry_pick(cx: &mut gpui::TestAppContext) {
    init_test(cx);
    cx.executor().allow_parking();

    let root = TempTree::new(json!({
        "project": {
            "a.txt": "a",
        },
    }));
    let root_path = root.path();

    let work_dir = &root_path.join("project");
    let repo = git_init(work_dir);
    git_add("a.txt", &repo);
    git_commit("init", &repo);

    let project = Project::test(Arc::new(RealFs::new(None, cx.executor())), [root_path], cx).await;

    let tree = project.read_with(cx, |project, cx| project.worktrees(cx).next().unwrap());
    tree.flush_fs_events(cx).await;
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.executor().run_until_parked();

    let repository = project.read_with(cx, |project, cx| {
        project.repositories(cx).values().next().unwrap().clone()
    });

    git_branch("other-branch", &repo);
    git_checkout("other-branch", &repo);
    std::fs::write(root_path.join("project/a.txt"), "A").unwrap();
    git_add("a.txt", &repo);
    git_commit("capitalize", &repo);
    let commit = git_rev_parse("HEAD", &repo);
    git_checkout("main", &repo);
    std::fs::write(root_path.join("project/a.txt"), "b").unwrap();
    git_add("a.txt", &repo);
    git_commit("improve letter", &repo);
    git_cherry_pick_expect_conflict(&commit, &repo);
    std::fs::read_to_string(root_path.join("project/.git/CHERRY_PICK_HEAD"))
        .expect("No CHERRY_PICK_HEAD");
    pretty_assertions::assert_eq!(
        git_status(&repo),
        collections::HashMap::from_iter([("a.txt".to_owned(), GIT_STATUS_CONFLICTED.to_owned())])
    );
    tree.flush_fs_events(cx).await;
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.executor().run_until_parked();
    let conflicts = repository.update(cx, |repository, _| {
        repository
            .merge_conflicts
            .iter()
            .cloned()
            .collect::<Vec<_>>()
    });
    pretty_assertions::assert_eq!(conflicts, [RepoPath::from("a.txt")]);

    git_add("a.txt", &repo);
    // Attempt to manually simulate what `git cherry-pick --continue` would do.
    git_commit("whatevs", &repo);
    std::fs::remove_file(root.path().join("project/.git/CHERRY_PICK_HEAD"))
        .expect("Failed to remove CHERRY_PICK_HEAD");
    pretty_assertions::assert_eq!(git_status(&repo), collections::HashMap::default());
    tree.flush_fs_events(cx).await;
    let conflicts = repository.update(cx, |repository, _| {
        repository
            .merge_conflicts
            .iter()
            .cloned()
            .collect::<Vec<_>>()
    });
    pretty_assertions::assert_eq!(conflicts, []);
}

#[gpui::test]
async fn test_update_gitignore(cx: &mut gpui::TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        path!("/root"),
        json!({
            ".git": {},
            ".gitignore": "*.txt\n",
            "a.xml": "<a></a>",
            "b.txt": "Some text"
        }),
    )
    .await;

    fs.set_head_and_index_for_repo(
        path!("/root/.git").as_ref(),
        &[
            (".gitignore", "*.txt\n".into()),
            ("a.xml", "<a></a>".into()),
        ],
    );

    let project = Project::test(fs.clone(), [path!("/root").as_ref()], cx).await;

    let tree = project.read_with(cx, |project, cx| project.worktrees(cx).next().unwrap());
    tree.flush_fs_events(cx).await;
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.executor().run_until_parked();

    let repository = project.read_with(cx, |project, cx| {
        project.repositories(cx).values().next().unwrap().clone()
    });

    // One file is unmodified, the other is ignored.
    cx.read(|cx| {
        assert_entry_git_state(tree.read(cx), repository.read(cx), "a.xml", None, false);
        assert_entry_git_state(tree.read(cx), repository.read(cx), "b.txt", None, true);
    });

    // Change the gitignore, and stage the newly non-ignored file.
    fs.atomic_write(path!("/root/.gitignore").into(), "*.xml\n".into())
        .await
        .unwrap();
    fs.set_index_for_repo(
        Path::new(path!("/root/.git")),
        &[
            (".gitignore", "*.txt\n".into()),
            ("a.xml", "<a></a>".into()),
            ("b.txt", "Some text".into()),
        ],
    );

    cx.executor().run_until_parked();
    cx.read(|cx| {
        assert_entry_git_state(tree.read(cx), repository.read(cx), "a.xml", None, true);
        assert_entry_git_state(
            tree.read(cx),
            repository.read(cx),
            "b.txt",
            Some(StatusCode::Added),
            false,
        );
    });
}

// NOTE:
// This test always fails on Windows, because on Windows, unlike on Unix, you can't rename
// a directory which some program has already open.
// This is a limitation of the Windows.
// See: https://stackoverflow.com/questions/41365318/access-is-denied-when-renaming-folder
// See: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/ns-ntifs-_file_rename_information
#[gpui::test]
#[cfg_attr(target_os = "windows", ignore)]
async fn test_rename_work_directory(cx: &mut gpui::TestAppContext) {
    init_test(cx);
    cx.executor().allow_parking();
    let root = TempTree::new(json!({
        "projects": {
            "project1": {
                "a": "",
                "b": "",
            }
        },

    }));
    let root_path = root.path();

    let repo = git_init(&root_path.join("projects/project1"));
    git_add("a", &repo);
    git_commit("init", &repo);
    std::fs::write(root_path.join("projects/project1/a"), "aa").unwrap();

    let project = Project::test(Arc::new(RealFs::new(None, cx.executor())), [root_path], cx).await;

    let tree = project.read_with(cx, |project, cx| project.worktrees(cx).next().unwrap());
    tree.flush_fs_events(cx).await;
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.executor().run_until_parked();
    let _buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer(root_path.join("projects/project1/a"), cx)
        })
        .await
        .unwrap();
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.executor().run_until_parked();

    let repository = project.read_with(cx, |project, cx| {
        project.repositories(cx).values().next().unwrap().clone()
    });

    repository.read_with(cx, |repository, _| {
        assert_eq!(
            repository.work_directory_abs_path.as_ref(),
            root_path.join("projects/project1").as_path()
        );
        assert_eq!(
            repository
                .status_for_path(&repo_path("a"))
                .map(|entry| entry.status),
            Some(StatusCode::Modified.worktree()),
        );
        assert_eq!(
            repository
                .status_for_path(&repo_path("b"))
                .map(|entry| entry.status),
            Some(FileStatus::Untracked),
        );
    });

    std::fs::rename(
        root_path.join("projects/project1"),
        root_path.join("projects/project2"),
    )
    .unwrap();
    tree.flush_fs_events(cx).await;
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.executor().run_until_parked();

    repository.read_with(cx, |repository, _| {
        assert_eq!(
            repository.work_directory_abs_path.as_ref(),
            root_path.join("projects/project2").as_path()
        );
        assert_eq!(
            repository.status_for_path(&repo_path("a")).unwrap().status,
            StatusCode::Modified.worktree(),
        );
        assert_eq!(
            repository.status_for_path(&repo_path("b")).unwrap().status,
            FileStatus::Untracked,
        );
    });
}

// NOTE: This test always fails on Windows, because on Windows, unlike on Unix,
// you can't rename a directory which some program has already open. This is a
// limitation of the Windows. See:
// See: https://stackoverflow.com/questions/41365318/access-is-denied-when-renaming-folder
// See: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/ns-ntifs-_file_rename_information
#[gpui::test]
#[cfg_attr(target_os = "windows", ignore)]
async fn test_file_status(cx: &mut gpui::TestAppContext) {
    init_test(cx);
    cx.executor().allow_parking();
    const IGNORE_RULE: &str = "**/target";

    let root = TempTree::new(json!({
        "project": {
            "a.txt": "a",
            "b.txt": "bb",
            "c": {
                "d": {
                    "e.txt": "eee"
                }
            },
            "f.txt": "ffff",
            "target": {
                "build_file": "???"
            },
            ".gitignore": IGNORE_RULE
        },

    }));
    let root_path = root.path();

    const A_TXT: &str = "a.txt";
    const B_TXT: &str = "b.txt";
    const E_TXT: &str = "c/d/e.txt";
    const F_TXT: &str = "f.txt";
    const DOTGITIGNORE: &str = ".gitignore";
    const BUILD_FILE: &str = "target/build_file";

    // Set up git repository before creating the worktree.
    let work_dir = root.path().join("project");
    let repo = git_init(work_dir.as_path());
    git_add(A_TXT, &repo);
    git_add(E_TXT, &repo);
    git_add(DOTGITIGNORE, &repo);
    git_commit("Initial commit", &repo);

    let project = Project::test(Arc::new(RealFs::new(None, cx.executor())), [root_path], cx).await;

    let tree = project.read_with(cx, |project, cx| project.worktrees(cx).next().unwrap());
    tree.flush_fs_events(cx).await;
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.executor().run_until_parked();

    let repository = project.read_with(cx, |project, cx| {
        project.repositories(cx).values().next().unwrap().clone()
    });

    // Check that the right git state is observed on startup
    repository.read_with(cx, |repository, _cx| {
        assert_eq!(
            repository.work_directory_abs_path.as_ref(),
            root_path.join("project").as_path()
        );

        assert_eq!(
            repository
                .status_for_path(&repo_path(B_TXT))
                .unwrap()
                .status,
            FileStatus::Untracked,
        );
        assert_eq!(
            repository
                .status_for_path(&repo_path(F_TXT))
                .unwrap()
                .status,
            FileStatus::Untracked,
        );
    });

    // Modify a file in the working copy.
    std::fs::write(work_dir.join(A_TXT), "aa").unwrap();
    tree.flush_fs_events(cx).await;
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.executor().run_until_parked();

    // The worktree detects that the file's git status has changed.
    repository.read_with(cx, |repository, _| {
        assert_eq!(
            repository
                .status_for_path(&repo_path(A_TXT))
                .unwrap()
                .status,
            StatusCode::Modified.worktree(),
        );
    });

    // Create a commit in the git repository.
    git_add(A_TXT, &repo);
    git_add(B_TXT, &repo);
    git_commit("Committing modified and added", &repo);
    tree.flush_fs_events(cx).await;
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.executor().run_until_parked();

    // The worktree detects that the files' git status have changed.
    repository.read_with(cx, |repository, _cx| {
        assert_eq!(
            repository
                .status_for_path(&repo_path(F_TXT))
                .unwrap()
                .status,
            FileStatus::Untracked,
        );
        assert_eq!(repository.status_for_path(&repo_path(B_TXT)), None);
        assert_eq!(repository.status_for_path(&repo_path(A_TXT)), None);
    });

    // Modify files in the working copy and perform git operations on other files.
    git_reset(0, &repo);
    git_remove_index(Path::new(B_TXT), &repo);
    git_stash(&repo);
    std::fs::write(work_dir.join(E_TXT), "eeee").unwrap();
    std::fs::write(work_dir.join(BUILD_FILE), "this should be ignored").unwrap();
    tree.flush_fs_events(cx).await;
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.executor().run_until_parked();

    // Check that more complex repo changes are tracked
    repository.read_with(cx, |repository, _cx| {
        assert_eq!(repository.status_for_path(&repo_path(A_TXT)), None);
        assert_eq!(
            repository
                .status_for_path(&repo_path(B_TXT))
                .unwrap()
                .status,
            FileStatus::Untracked,
        );
        assert_eq!(
            repository
                .status_for_path(&repo_path(E_TXT))
                .unwrap()
                .status,
            StatusCode::Modified.worktree(),
        );
    });

    std::fs::remove_file(work_dir.join(B_TXT)).unwrap();
    std::fs::remove_dir_all(work_dir.join("c")).unwrap();
    std::fs::write(
        work_dir.join(DOTGITIGNORE),
        [IGNORE_RULE, "f.txt"].join("\n"),
    )
    .unwrap();

    git_add(Path::new(DOTGITIGNORE), &repo);
    git_commit("Committing modified git ignore", &repo);

    tree.flush_fs_events(cx).await;
    cx.executor().run_until_parked();

    let mut renamed_dir_name = "first_directory/second_directory";
    const RENAMED_FILE: &str = "rf.txt";

    std::fs::create_dir_all(work_dir.join(renamed_dir_name)).unwrap();
    std::fs::write(
        work_dir.join(renamed_dir_name).join(RENAMED_FILE),
        "new-contents",
    )
    .unwrap();

    tree.flush_fs_events(cx).await;
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.executor().run_until_parked();

    repository.read_with(cx, |repository, _cx| {
        assert_eq!(
            repository
                .status_for_path(&RepoPath::from_rel_path(
                    &rel_path(renamed_dir_name).join(rel_path(RENAMED_FILE))
                ))
                .unwrap()
                .status,
            FileStatus::Untracked,
        );
    });

    renamed_dir_name = "new_first_directory/second_directory";

    std::fs::rename(
        work_dir.join("first_directory"),
        work_dir.join("new_first_directory"),
    )
    .unwrap();

    tree.flush_fs_events(cx).await;
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.executor().run_until_parked();

    repository.read_with(cx, |repository, _cx| {
        assert_eq!(
            repository
                .status_for_path(&RepoPath::from_rel_path(
                    &rel_path(renamed_dir_name).join(rel_path(RENAMED_FILE))
                ))
                .unwrap()
                .status,
            FileStatus::Untracked,
        );
    });
}

#[gpui::test]
async fn test_bare_dot_git_changed_event_refreshes_git_state(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/repo"),
        json!({
            ".git": {},
            "file.txt": "new contents",
        }),
    )
    .await;

    let dot_git = Path::new(path!("/repo/.git"));
    fs.set_head_for_repo(dot_git, &[("file.txt", "old contents".into())], "old-sha");
    fs.set_index_for_repo(dot_git, &[("file.txt", "old contents".into())]);

    let project = Project::test(fs.clone(), [path!("/repo").as_ref()], cx).await;
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.run_until_parked();

    let repository = project.read_with(cx, |project, cx| {
        project.repositories(cx).values().next().unwrap().clone()
    });

    repository.read_with(cx, |repository, _| {
        assert_eq!(
            repository
                .head_commit
                .as_ref()
                .map(|commit| commit.sha.as_ref()),
            Some("old-sha")
        );
        assert_eq!(
            repository
                .status_for_path(&repo_path("file.txt"))
                .map(|entry| entry.status),
            Some(StatusCode::Modified.worktree())
        );
    });

    fs.with_git_state(dot_git, false, |state| {
        state
            .head_contents
            .insert(repo_path("file.txt"), "new contents".into());
        state
            .index_contents
            .insert(repo_path("file.txt"), "new contents".into());
        state.refs.insert("HEAD".into(), "new-sha".into());
    })
    .unwrap();
    fs.emit_fs_event(dot_git, Some(PathEventKind::Changed));

    cx.run_until_parked();
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.run_until_parked();

    repository.read_with(cx, |repository, _| {
        assert_eq!(
            repository
                .head_commit
                .as_ref()
                .map(|commit| commit.sha.as_ref()),
            Some("new-sha")
        );
        assert_eq!(repository.status_for_path(&repo_path("file.txt")), None);
    });
}

#[gpui::test]
#[ignore]
async fn test_ignored_dirs_events(cx: &mut gpui::TestAppContext) {
    init_test(cx);
    cx.executor().allow_parking();

    const IGNORE_RULE: &str = "**/target";

    let root = TempTree::new(json!({
        "project": {
            "src": {
                "main.rs": "fn main() {}"
            },
            "target": {
                "debug": {
                    "important_text.txt": "important text",
                },
            },
            ".gitignore": IGNORE_RULE
        },

    }));
    let root_path = root.path();

    // Set up git repository before creating the worktree.
    let work_dir = root.path().join("project");
    let repo = git_init(work_dir.as_path());
    git_add("src/main.rs", &repo);
    git_add(".gitignore", &repo);
    git_commit("Initial commit", &repo);

    let project = Project::test(Arc::new(RealFs::new(None, cx.executor())), [root_path], cx).await;
    let repository_updates = Arc::new(Mutex::new(Vec::new()));
    let project_events = Arc::new(Mutex::new(Vec::new()));
    project.update(cx, |project, cx| {
        let repo_events = repository_updates.clone();
        cx.subscribe(project.git_store(), move |_, _, e, _| {
            if let GitStoreEvent::RepositoryUpdated(_, e, _) = e {
                repo_events.lock().push(e.clone());
            }
        })
        .detach();
        let project_events = project_events.clone();
        cx.subscribe_self(move |_, e, _| {
            if let Event::WorktreeUpdatedEntries(_, updates) = e {
                project_events.lock().extend(
                    updates
                        .iter()
                        .map(|(path, _, change)| (path.as_unix_str().to_string(), *change))
                        .filter(|(path, _)| path != "fs-event-sentinel"),
                );
            }
        })
        .detach();
    });

    let tree = project.read_with(cx, |project, cx| project.worktrees(cx).next().unwrap());
    tree.flush_fs_events(cx).await;
    tree.update(cx, |tree, cx| {
        tree.load_file(rel_path("project/target/debug/important_text.txt"), cx)
    })
    .await
    .unwrap();
    tree.update(cx, |tree, _| {
        assert_eq!(
            tree.entries(true, 0)
                .map(|entry| (entry.path.as_ref(), entry.is_ignored))
                .collect::<Vec<_>>(),
            vec![
                (rel_path(""), false),
                (rel_path("project/"), false),
                (rel_path("project/.gitignore"), false),
                (rel_path("project/src"), false),
                (rel_path("project/src/main.rs"), false),
                (rel_path("project/target"), true),
                (rel_path("project/target/debug"), true),
                (rel_path("project/target/debug/important_text.txt"), true),
            ]
        );
    });

    assert_eq!(
        repository_updates.lock().drain(..).collect::<Vec<_>>(),
        vec![RepositoryEvent::StatusesChanged,],
        "Initial worktree scan should produce a repo update event"
    );
    assert_eq!(
        project_events.lock().drain(..).collect::<Vec<_>>(),
        vec![
            ("project/target".to_string(), PathChange::Loaded),
            ("project/target/debug".to_string(), PathChange::Loaded),
            (
                "project/target/debug/important_text.txt".to_string(),
                PathChange::Loaded
            ),
        ],
        "Initial project changes should show that all not-ignored and all opened files are loaded"
    );

    let deps_dir = work_dir.join("target").join("debug").join("deps");
    std::fs::create_dir_all(&deps_dir).unwrap();
    tree.flush_fs_events(cx).await;
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.executor().run_until_parked();
    std::fs::write(deps_dir.join("aa.tmp"), "something tmp").unwrap();
    tree.flush_fs_events(cx).await;
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.executor().run_until_parked();
    std::fs::remove_dir_all(&deps_dir).unwrap();
    tree.flush_fs_events(cx).await;
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.executor().run_until_parked();

    tree.update(cx, |tree, _| {
        assert_eq!(
            tree.entries(true, 0)
                .map(|entry| (entry.path.as_ref(), entry.is_ignored))
                .collect::<Vec<_>>(),
            vec![
                (rel_path(""), false),
                (rel_path("project/"), false),
                (rel_path("project/.gitignore"), false),
                (rel_path("project/src"), false),
                (rel_path("project/src/main.rs"), false),
                (rel_path("project/target"), true),
                (rel_path("project/target/debug"), true),
                (rel_path("project/target/debug/important_text.txt"), true),
            ],
            "No stray temp files should be left after the flycheck changes"
        );
    });

    assert_eq!(
        repository_updates
            .lock()
            .iter()
            .cloned()
            .collect::<Vec<_>>(),
        Vec::new(),
        "No further RepositoryUpdated events should happen, as only ignored dirs' contents was changed",
    );
    assert_eq!(
        project_events.lock().as_slice(),
        vec![
            ("project/target/debug/deps".to_string(), PathChange::Added),
            ("project/target/debug/deps".to_string(), PathChange::Removed),
        ],
        "Due to `debug` directory being tracked, it should get updates for entries inside it.
        No updates for more nested directories should happen as those are ignored",
    );
}

// todo(jk): turning this test off until we rework it in such a way so that it is not so susceptible
// to different timings/ordering of events.
#[ignore]
#[gpui::test]
async fn test_odd_events_for_ignored_dirs(
    executor: BackgroundExecutor,
    cx: &mut gpui::TestAppContext,
) {
    init_test(cx);
    let fs = FakeFs::new(executor);
    fs.insert_tree(
        path!("/root"),
        json!({
            ".git": {},
            ".gitignore": "**/target/",
            "src": {
                "main.rs": "fn main() {}",
            },
            "target": {
                "debug": {
                    "foo.txt": "foo",
                    "deps": {}
                }
            }
        }),
    )
    .await;
    fs.set_head_and_index_for_repo(
        path!("/root/.git").as_ref(),
        &[
            (".gitignore", "**/target/".into()),
            ("src/main.rs", "fn main() {}".into()),
        ],
    );

    let project = Project::test(fs.clone(), [path!("/root").as_ref()], cx).await;
    let repository_updates = Arc::new(Mutex::new(Vec::new()));
    let project_events = Arc::new(Mutex::new(Vec::new()));
    project.update(cx, |project, cx| {
        let repository_updates = repository_updates.clone();
        cx.subscribe(project.git_store(), move |_, _, e, _| {
            if let GitStoreEvent::RepositoryUpdated(_, e, _) = e {
                repository_updates.lock().push(e.clone());
            }
        })
        .detach();
        let project_events = project_events.clone();
        cx.subscribe_self(move |_, e, _| {
            if let Event::WorktreeUpdatedEntries(_, updates) = e {
                project_events.lock().extend(
                    updates
                        .iter()
                        .map(|(path, _, change)| (path.as_unix_str().to_string(), *change))
                        .filter(|(path, _)| path != "fs-event-sentinel"),
                );
            }
        })
        .detach();
    });

    let tree = project.read_with(cx, |project, cx| project.worktrees(cx).next().unwrap());
    tree.update(cx, |tree, cx| {
        tree.load_file(rel_path("target/debug/foo.txt"), cx)
    })
    .await
    .unwrap();
    tree.flush_fs_events(cx).await;
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.run_until_parked();
    tree.update(cx, |tree, _| {
        assert_eq!(
            tree.entries(true, 0)
                .map(|entry| (entry.path.as_ref(), entry.is_ignored))
                .collect::<Vec<_>>(),
            vec![
                (rel_path(""), false),
                (rel_path(".gitignore"), false),
                (rel_path("src"), false),
                (rel_path("src/main.rs"), false),
                (rel_path("target"), true),
                (rel_path("target/debug"), true),
                (rel_path("target/debug/deps"), true),
                (rel_path("target/debug/foo.txt"), true),
            ]
        );
    });

    assert_eq!(
        repository_updates.lock().drain(..).collect::<Vec<_>>(),
        vec![
            RepositoryEvent::HeadChanged,
            RepositoryEvent::StatusesChanged,
            RepositoryEvent::StatusesChanged,
        ],
        "Initial worktree scan should produce a repo update event"
    );
    assert_eq!(
        project_events.lock().drain(..).collect::<Vec<_>>(),
        vec![
            ("target".to_string(), PathChange::Loaded),
            ("target/debug".to_string(), PathChange::Loaded),
            ("target/debug/deps".to_string(), PathChange::Loaded),
            ("target/debug/foo.txt".to_string(), PathChange::Loaded),
        ],
        "All non-ignored entries and all opened firs should be getting a project event",
    );

    // Emulate a flycheck spawn: it emits a `INODE_META_MOD`-flagged FS event on target/debug/deps, then creates and removes temp files inside.
    // This may happen multiple times during a single flycheck, but once is enough for testing.
    fs.emit_fs_event("/root/target/debug/deps", None);
    tree.flush_fs_events(cx).await;
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.executor().run_until_parked();

    assert_eq!(
        repository_updates
            .lock()
            .iter()
            .cloned()
            .collect::<Vec<_>>(),
        Vec::new(),
        "No further RepositoryUpdated events should happen, as only ignored dirs received FS events",
    );
    assert_eq!(
        project_events.lock().as_slice(),
        Vec::new(),
        "No further project events should happen, as only ignored dirs received FS events",
    );
}

#[gpui::test]
async fn test_repos_in_invisible_worktrees(
    executor: BackgroundExecutor,
    cx: &mut gpui::TestAppContext,
) {
    init_test(cx);
    let fs = FakeFs::new(executor);
    fs.insert_tree(
        path!("/root"),
        json!({
            "dir1": {
                ".git": {},
                "dep1": {
                    ".git": {},
                    "src": {
                        "a.txt": "",
                    },
                },
                "b.txt": "",
            },
        }),
    )
    .await;

    let project = Project::test(fs.clone(), [path!("/root/dir1/dep1").as_ref()], cx).await;
    let _visible_worktree =
        project.read_with(cx, |project, cx| project.worktrees(cx).next().unwrap());
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;

    let repos = project.read_with(cx, |project, cx| {
        project
            .repositories(cx)
            .values()
            .map(|repo| repo.read(cx).work_directory_abs_path.clone())
            .collect::<Vec<_>>()
    });
    pretty_assertions::assert_eq!(repos, [Path::new(path!("/root/dir1/dep1")).into()]);

    let (_invisible_worktree, _) = project
        .update(cx, |project, cx| {
            project.worktree_store().update(cx, |worktree_store, cx| {
                worktree_store.find_or_create_worktree(path!("/root/dir1/b.txt"), false, cx)
            })
        })
        .await
        .expect("failed to create worktree");
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;

    let repos = project.read_with(cx, |project, cx| {
        project
            .repositories(cx)
            .values()
            .map(|repo| repo.read(cx).work_directory_abs_path.clone())
            .collect::<Vec<_>>()
    });
    pretty_assertions::assert_eq!(repos, [Path::new(path!("/root/dir1/dep1")).into()]);
}

#[gpui::test(iterations = 10)]
async fn test_rescan_with_gitignore(cx: &mut gpui::TestAppContext) {
    init_test(cx);
    cx.update(|cx| {
        cx.update_global::<SettingsStore, _>(|store, cx| {
            store.update_user_settings(cx, |settings| {
                settings.project.worktree.file_scan_exclusions =
                    Some(SplicingVec::from(Vec::new()));
            });
        });
    });
    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        path!("/root"),
        json!({
            ".gitignore": "ancestor-ignored-file1\nancestor-ignored-file2\n",
            "tree": {
                ".git": {},
                ".gitignore": "ignored-dir\n",
                "tracked-dir": {
                    "tracked-file1": "",
                    "ancestor-ignored-file1": "",
                },
                "ignored-dir": {
                    "ignored-file1": ""
                }
            }
        }),
    )
    .await;
    fs.set_head_and_index_for_repo(
        path!("/root/tree/.git").as_ref(),
        &[
            (".gitignore", "ignored-dir\n".into()),
            ("tracked-dir/tracked-file1", "".into()),
        ],
    );

    let project = Project::test(fs.clone(), [path!("/root/tree").as_ref()], cx).await;

    let tree = project.read_with(cx, |project, cx| project.worktrees(cx).next().unwrap());
    tree.flush_fs_events(cx).await;
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.executor().run_until_parked();

    let repository = project.read_with(cx, |project, cx| {
        project.repositories(cx).values().next().unwrap().clone()
    });

    tree.read_with(cx, |tree, _| {
        tree.as_local()
            .unwrap()
            .manually_refresh_entries_for_paths(vec![rel_path("ignored-dir").into()])
    })
    .recv()
    .await;

    cx.read(|cx| {
        assert_entry_git_state(
            tree.read(cx),
            repository.read(cx),
            "tracked-dir/tracked-file1",
            None,
            false,
        );
        assert_entry_git_state(
            tree.read(cx),
            repository.read(cx),
            "tracked-dir/ancestor-ignored-file1",
            None,
            false,
        );
        assert_entry_git_state(
            tree.read(cx),
            repository.read(cx),
            "ignored-dir/ignored-file1",
            None,
            true,
        );
    });

    fs.create_file(
        path!("/root/tree/tracked-dir/tracked-file2").as_ref(),
        Default::default(),
    )
    .await
    .unwrap();
    fs.set_index_for_repo(
        path!("/root/tree/.git").as_ref(),
        &[
            (".gitignore", "ignored-dir\n".into()),
            ("tracked-dir/tracked-file1", "".into()),
            ("tracked-dir/tracked-file2", "".into()),
        ],
    );
    fs.create_file(
        path!("/root/tree/tracked-dir/ancestor-ignored-file2").as_ref(),
        Default::default(),
    )
    .await
    .unwrap();
    fs.create_file(
        path!("/root/tree/ignored-dir/ignored-file2").as_ref(),
        Default::default(),
    )
    .await
    .unwrap();

    cx.executor().run_until_parked();
    cx.read(|cx| {
        assert_entry_git_state(
            tree.read(cx),
            repository.read(cx),
            "tracked-dir/tracked-file2",
            Some(StatusCode::Added),
            false,
        );
        assert_entry_git_state(
            tree.read(cx),
            repository.read(cx),
            "tracked-dir/ancestor-ignored-file2",
            None,
            false,
        );
        assert_entry_git_state(
            tree.read(cx),
            repository.read(cx),
            "ignored-dir/ignored-file2",
            None,
            true,
        );
        assert!(
            tree.read(cx)
                .entry_for_path(&rel_path(".git"))
                .unwrap()
                .is_ignored
        );
    });
}

#[gpui::test]
async fn test_git_worktrees_and_submodules(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/project"),
        json!({
            ".git": {
                "worktrees": {
                    "some-worktree": {
                        "commondir": "../..\n",
                        // For is_git_dir
                        "HEAD": "",
                        "config": ""
                    }
                },
                "modules": {
                    "subdir": {
                        "some-submodule": {
                            // For is_git_dir
                            "HEAD": "",
                            "config": "",
                        }
                    }
                }
            },
            "src": {
                "a.txt": "A",
            },
            "some-worktree": {
                ".git": "gitdir: ../.git/worktrees/some-worktree\n",
                "src": {
                    "b.txt": "B",
                }
            },
            "subdir": {
                "some-submodule": {
                    ".git": "gitdir: ../../.git/modules/subdir/some-submodule\n",
                    "c.txt": "C",
                }
            }
        }),
    )
    .await;

    let project = Project::test(fs.clone(), [path!("/project").as_ref()], cx).await;
    let scan_complete = project.update(cx, |project, cx| project.git_scans_complete(cx));
    scan_complete.await;

    let mut repositories = project.update(cx, |project, cx| {
        project
            .repositories(cx)
            .values()
            .map(|repo| repo.read(cx).work_directory_abs_path.clone())
            .collect::<Vec<_>>()
    });
    repositories.sort();
    pretty_assertions::assert_eq!(
        repositories,
        [
            Path::new(path!("/project")).into(),
            Path::new(path!("/project/some-worktree")).into(),
            Path::new(path!("/project/subdir/some-submodule")).into(),
        ]
    );

    // Generate a git-related event for the worktree and check that it's refreshed.
    fs.with_git_state(
        path!("/project/some-worktree/.git").as_ref(),
        true,
        |state| {
            state
                .head_contents
                .insert(repo_path("src/b.txt"), b"b".to_vec());
            state
                .index_contents
                .insert(repo_path("src/b.txt"), b"b".to_vec());
        },
    )
    .unwrap();
    cx.run_until_parked();

    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer(path!("/project/some-worktree/src/b.txt"), cx)
        })
        .await
        .unwrap();
    let (worktree_repo, barrier) = project.update(cx, |project, cx| {
        let (repo, _) = project
            .git_store()
            .read(cx)
            .repository_and_path_for_buffer_id(buffer.read(cx).remote_id(), cx)
            .unwrap();
        pretty_assertions::assert_eq!(
            repo.read(cx).work_directory_abs_path,
            Path::new(path!("/project/some-worktree")).into(),
        );
        pretty_assertions::assert_eq!(
            repo.read(cx).main_worktree_abs_path(),
            Some(Path::new(path!("/project"))),
        );
        assert!(
            repo.read(cx).linked_worktree_path().is_some(),
            "linked worktree should be detected as a linked worktree"
        );
        let barrier = repo.update(cx, |repo, _| repo.barrier());
        (repo.clone(), barrier)
    });
    barrier.await.unwrap();
    worktree_repo.update(cx, |repo, _| {
        pretty_assertions::assert_eq!(
            repo.status_for_path(&repo_path("src/b.txt"))
                .unwrap()
                .status,
            StatusCode::Modified.worktree(),
        );
    });

    // The same for the submodule.
    fs.with_git_state(
        path!("/project/subdir/some-submodule/.git").as_ref(),
        true,
        |state| {
            state
                .head_contents
                .insert(repo_path("c.txt"), b"c".to_vec());
            state
                .index_contents
                .insert(repo_path("c.txt"), b"c".to_vec());
        },
    )
    .unwrap();
    cx.run_until_parked();

    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer(path!("/project/subdir/some-submodule/c.txt"), cx)
        })
        .await
        .unwrap();
    let (submodule_repo, barrier) = project.update(cx, |project, cx| {
        let (repo, _) = project
            .git_store()
            .read(cx)
            .repository_and_path_for_buffer_id(buffer.read(cx).remote_id(), cx)
            .unwrap();
        pretty_assertions::assert_eq!(
            repo.read(cx).work_directory_abs_path,
            Path::new(path!("/project/subdir/some-submodule")).into(),
        );
        pretty_assertions::assert_eq!(
            repo.read(cx).main_worktree_abs_path(),
            Some(Path::new(path!("/project/subdir/some-submodule"))),
        );
        assert!(
            repo.read(cx).linked_worktree_path().is_none(),
            "submodule should not be detected as a linked worktree"
        );
        let barrier = repo.update(cx, |repo, _| repo.barrier());
        (repo.clone(), barrier)
    });
    barrier.await.unwrap();
    submodule_repo.update(cx, |repo, _| {
        pretty_assertions::assert_eq!(
            repo.status_for_path(&repo_path("c.txt")).unwrap().status,
            StatusCode::Modified.worktree(),
        );
    });
}

#[gpui::test]
async fn test_repository_deduplication(cx: &mut gpui::TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        path!("/root"),
        json!({
            "project": {
                ".git": {},
                "child1": {
                    "a.txt": "A",
                },
                "child2": {
                    "b.txt": "B",
                }
            }
        }),
    )
    .await;

    let project = Project::test(
        fs.clone(),
        [
            path!("/root/project/child1").as_ref(),
            path!("/root/project/child2").as_ref(),
        ],
        cx,
    )
    .await;

    let tree = project.read_with(cx, |project, cx| project.worktrees(cx).next().unwrap());
    tree.flush_fs_events(cx).await;
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.executor().run_until_parked();

    let repos = project.read_with(cx, |project, cx| {
        project
            .repositories(cx)
            .values()
            .map(|repo| repo.read(cx).work_directory_abs_path.clone())
            .collect::<Vec<_>>()
    });
    pretty_assertions::assert_eq!(repos, [Path::new(path!("/root/project")).into()]);
}

#[gpui::test]
async fn test_buffer_changed_file_path_updates_git_diff(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let file_1_committed = String::from(r#"file_1_committed"#);
    let file_1_staged = String::from(r#"file_1_staged"#);
    let file_2_committed = String::from(r#"file_2_committed"#);
    let file_2_staged = String::from(r#"file_2_staged"#);
    let buffer_contents = String::from(r#"buffer"#);

    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        path!("/dir"),
        json!({
            ".git": {},
           "src": {
               "file_1.rs": file_1_committed.clone(),
               "file_2.rs": file_2_committed.clone(),
           }
        }),
    )
    .await;

    fs.set_head_for_repo(
        path!("/dir/.git").as_ref(),
        &[
            ("src/file_1.rs", file_1_committed.clone()),
            ("src/file_2.rs", file_2_committed.clone()),
        ],
        "deadbeef",
    );
    fs.set_index_for_repo(
        path!("/dir/.git").as_ref(),
        &[
            ("src/file_1.rs", file_1_staged.clone()),
            ("src/file_2.rs", file_2_staged.clone()),
        ],
    );

    let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;

    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer(path!("/dir/src/file_1.rs"), cx)
        })
        .await
        .unwrap();

    buffer.update(cx, |buffer, cx| {
        buffer.edit([(0..buffer.len(), buffer_contents.as_str())], None, cx);
    });

    let unstaged_diff = project
        .update(cx, |project, cx| {
            project.open_unstaged_diff(buffer.clone(), cx)
        })
        .await
        .unwrap();

    cx.run_until_parked();

    unstaged_diff.update(cx, |unstaged_diff, cx| {
        let base_text = unstaged_diff.base_text_string(cx).unwrap();
        assert_eq!(base_text, file_1_staged, "Should start with file_1 staged");
    });

    // Save the buffer as `file_2.rs`, which should trigger the
    // `BufferChangedFilePath` event.
    project
        .update(cx, |project, cx| {
            let worktree_id = project.worktrees(cx).next().unwrap().read(cx).id();
            let path = ProjectPath {
                worktree_id,
                path: rel_path("src/file_2.rs").into(),
            };
            project.save_buffer_as(buffer.clone(), path, cx)
        })
        .await
        .unwrap();

    cx.run_until_parked();

    // Verify that the diff bases have been updated to file_2's contents due to
    // the `BufferChangedFilePath` event being handled.
    unstaged_diff.update(cx, |unstaged_diff, cx| {
        let snapshot = buffer.read(cx).snapshot();
        let base_text = unstaged_diff.base_text_string(cx).unwrap();
        assert_eq!(
            base_text, file_2_staged,
            "Diff bases should be automatically updated to file_2 staged content"
        );

        let hunks: Vec<_> = unstaged_diff.snapshot(cx).hunks(&snapshot).collect();
        assert!(!hunks.is_empty(), "Should have diff hunks for file_2");
    });

    let uncommitted_diff = project
        .update(cx, |project, cx| {
            project.open_uncommitted_diff(buffer.clone(), cx)
        })
        .await
        .unwrap();

    cx.run_until_parked();

    uncommitted_diff.update(cx, |uncommitted_diff, cx| {
        let base_text = uncommitted_diff.base_text_string(cx).unwrap();
        assert_eq!(
            base_text, file_2_committed,
            "Uncommitted diff should compare against file_2 committed content"
        );
    });
}

async fn search(
    project: &Entity<Project>,
    query: SearchQuery,
    cx: &mut gpui::TestAppContext,
) -> Result<HashMap<String, Vec<Range<usize>>>> {
    let search_rx = project.update(cx, |project, cx| project.search(query, cx));
    let mut results = HashMap::default();
    while let Ok(search_result) = search_rx.rx.recv().await {
        match search_result {
            SearchResult::Buffer { buffer, ranges } => {
                results.entry(buffer).or_insert(ranges);
            }
            SearchResult::LimitReached | SearchResult::WaitingForScan | SearchResult::Searching => {
            }
        }
    }
    Ok(results
        .into_iter()
        .map(|(buffer, ranges)| {
            buffer.update(cx, |buffer, cx| {
                let path = buffer
                    .file()
                    .unwrap()
                    .full_path(cx)
                    .to_string_lossy()
                    .to_string();
                let ranges = ranges
                    .into_iter()
                    .map(|range| range.to_offset(buffer))
                    .collect::<Vec<_>>();
                (path, ranges)
            })
        })
        .collect())
}

#[gpui::test]
async fn test_undo_encoding_change(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());

    // Create a file with ASCII content "Hi" - this will be detected as UTF-8
    // When reinterpreted as UTF-16LE, the bytes 0x48 0x69 become a single character
    let ascii_bytes: Vec<u8> = vec![0x48, 0x69];
    fs.insert_tree(path!("/dir"), json!({})).await;
    fs.insert_file(path!("/dir/test.txt"), ascii_bytes).await;

    let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;

    let buffer = project
        .update(cx, |p, cx| p.open_local_buffer(path!("/dir/test.txt"), cx))
        .await
        .unwrap();

    let (initial_encoding, initial_text, initial_dirty) = buffer.read_with(cx, |buffer, _| {
        (buffer.encoding(), buffer.text(), buffer.is_dirty())
    });
    assert_eq!(initial_encoding, encoding_rs::UTF_8);
    assert_eq!(initial_text, "Hi");
    assert!(!initial_dirty);

    let reload_receiver = buffer.update(cx, |buffer, cx| {
        buffer.reload_with_encoding(encoding_rs::UTF_16LE, cx)
    });
    cx.executor().run_until_parked();

    // Wait for reload to complete
    let _ = reload_receiver.await;

    // Verify the encoding changed, text is different, and still not dirty (we reloaded from disk)
    let (reloaded_encoding, reloaded_text, reloaded_dirty) = buffer.read_with(cx, |buffer, _| {
        (buffer.encoding(), buffer.text(), buffer.is_dirty())
    });
    assert_eq!(reloaded_encoding, encoding_rs::UTF_16LE);
    assert_eq!(reloaded_text, "楈");
    assert!(!reloaded_dirty);

    // Undo the reload
    buffer.update(cx, |buffer, cx| {
        buffer.undo(cx);
    });

    buffer.read_with(cx, |buffer, _| {
        assert_eq!(buffer.encoding(), encoding_rs::UTF_8);
        assert_eq!(buffer.text(), "Hi");
        assert!(!buffer.is_dirty());
    });

    buffer.update(cx, |buffer, cx| {
        buffer.redo(cx);
    });

    buffer.read_with(cx, |buffer, _| {
        assert_eq!(buffer.encoding(), encoding_rs::UTF_16LE);
        assert_ne!(buffer.text(), "Hi");
        assert!(!buffer.is_dirty());
    });
}

#[gpui::test]
async fn test_initial_scan_complete(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/root"),
        json!({
            "a": {
                ".git": {},
                ".zed": {
                    "tasks.json": r#"[{"label": "task-a", "command": "echo a"}]"#
                },
                "src": { "main.rs": "" }
            },
            "b": {
                ".git": {},
                ".zed": {
                    "tasks.json": r#"[{"label": "task-b", "command": "echo b"}]"#
                },
                "src": { "lib.rs": "" }
            },
        }),
    )
    .await;

    let repos_created = Rc::new(RefCell::new(Vec::new()));
    let _observe = {
        let repos_created = repos_created.clone();
        cx.update(|cx| {
            cx.observe_new::<Repository>(move |repo, _, cx| {
                repos_created.borrow_mut().push(cx.entity().downgrade());
                let _ = repo;
            })
        })
    };

    let project = Project::test(
        fs.clone(),
        [path!("/root/a").as_ref(), path!("/root/b").as_ref()],
        cx,
    )
    .await;

    let scan_complete = project.read_with(cx, |project, cx| project.wait_for_initial_scan(cx));
    scan_complete.await;

    project.read_with(cx, |project, cx| {
        assert!(
            project.worktree_store().read(cx).initial_scan_completed(),
            "Expected initial scan to be completed after awaiting wait_for_initial_scan"
        );
    });

    let created_repos_len = repos_created.borrow().len();
    assert_eq!(
        created_repos_len, 2,
        "Expected 2 repositories to be created during scan, got {}",
        created_repos_len
    );

    project.read_with(cx, |project, cx| {
        let git_store = project.git_store().read(cx);
        assert_eq!(
            git_store.repositories().len(),
            2,
            "Expected 2 repositories in GitStore"
        );
    });
}

pub fn init_test(cx: &mut gpui::TestAppContext) {
    zlog::init_test();

    cx.update(|cx| {
        let settings_store = SettingsStore::test(cx);
        cx.set_global(settings_store);
        release_channel::init(semver::Version::new(0, 0, 0), cx);
    });
}

fn json_lang() -> Arc<Language> {
    Arc::new(Language::new(
        LanguageConfig {
            name: "JSON".into(),
            matcher: (LanguageMatcher {
                path_suffixes: vec!["json".to_string()],
                ..Default::default()
            })
            .into(),
            ..Default::default()
        },
        None,
    ))
}

fn js_lang() -> Arc<Language> {
    Arc::new(Language::new(
        LanguageConfig {
            name: "JavaScript".into(),
            matcher: (LanguageMatcher {
                path_suffixes: vec!["js".to_string()],
                ..Default::default()
            })
            .into(),
            ..Default::default()
        },
        None,
    ))
}

fn get_all_tasks(
    project: &Entity<Project>,
    task_contexts: Arc<TaskContexts>,
    cx: &mut App,
) -> Task<Vec<(TaskSourceKind, ResolvedTask)>> {
    let new_tasks = project.update(cx, |project, cx| {
        project.task_store().update(cx, |task_store, cx| {
            task_store.task_inventory().unwrap().update(cx, |this, cx| {
                this.used_and_current_resolved_tasks(task_contexts, cx)
            })
        })
    });

    cx.background_spawn(async move {
        let (mut old, new) = new_tasks.await;
        old.extend(new);
        old
    })
}

#[track_caller]
fn assert_entry_git_state(
    tree: &Worktree,
    repository: &Repository,
    path: &str,
    index_status: Option<StatusCode>,
    is_ignored: bool,
) {
    assert_eq!(tree.abs_path(), repository.work_directory_abs_path);
    let entry = tree
        .entry_for_path(&rel_path(path))
        .unwrap_or_else(|| panic!("entry {path} not found"));
    let status = repository
        .status_for_path(&repo_path(path))
        .map(|entry| entry.status);
    let expected = index_status.map(|index_status| {
        TrackedStatus {
            index_status,
            worktree_status: StatusCode::Unmodified,
        }
        .into()
    });
    assert_eq!(
        status, expected,
        "expected {path} to have git status: {expected:?}"
    );
    assert_eq!(
        entry.is_ignored, is_ignored,
        "expected {path} to have is_ignored: {is_ignored}"
    );
}

#[cfg(any())]
const GIT_STATUS_CONFLICTED: &str = "UU";

#[allow(clippy::disallowed_methods)]
fn git_cmd(work_dir: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(work_dir)
        .env("GIT_CONFIG_GLOBAL", "")
        .env("GIT_CONFIG_SYSTEM", "")
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@zed.dev")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@zed.dev");
    cmd
}

#[allow(clippy::disallowed_methods)]
#[track_caller]
fn git_init(path: &Path) -> PathBuf {
    let output = git_cmd(path)
        .args(["init", "-b", "main"])
        .output()
        .expect("Failed to run git init");
    assert!(
        output.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    path.to_path_buf()
}

#[allow(clippy::disallowed_methods)]
#[track_caller]
fn git_add<P: AsRef<Path>>(path: P, work_dir: &Path) {
    let output = git_cmd(work_dir)
        .args(["add"])
        .arg(path.as_ref())
        .output()
        .expect("Failed to run git add");
    assert!(
        output.status.success(),
        "git add failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[allow(clippy::disallowed_methods)]
#[track_caller]
fn git_remove_index(path: &Path, work_dir: &Path) {
    let output = git_cmd(work_dir)
        .args(["rm", "--cached"])
        .arg(path)
        .output()
        .expect("Failed to run git rm");
    assert!(
        output.status.success(),
        "git rm --cached failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[allow(clippy::disallowed_methods)]
#[track_caller]
fn git_commit(msg: &str, work_dir: &Path) {
    let output = git_cmd(work_dir)
        .args(["commit", "-m", msg])
        .output()
        .expect("Failed to run git commit");
    assert!(
        output.status.success(),
        "git commit failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[allow(clippy::disallowed_methods)]
#[track_caller]
fn git_stash(work_dir: &Path) {
    let output = git_cmd(work_dir)
        .args(["stash"])
        .output()
        .expect("Failed to run git stash");
    assert!(
        output.status.success(),
        "git stash failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[allow(clippy::disallowed_methods)]
#[track_caller]
fn git_reset(offset: usize, work_dir: &Path) {
    let target = format!("HEAD~{}", offset + 1);
    let output = git_cmd(work_dir)
        .args(["reset", "--soft", &target])
        .output()
        .expect("Failed to run git reset");
    assert!(
        output.status.success(),
        "git reset failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(target_os = "linux")]
#[allow(clippy::disallowed_methods)]
#[track_caller]
fn git_branch(name: &str, work_dir: &Path) {
    let output = git_cmd(work_dir)
        .args(["branch", name])
        .output()
        .expect("Failed to run git branch");
    assert!(
        output.status.success(),
        "git branch failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(target_os = "linux")]
#[allow(clippy::disallowed_methods)]
#[track_caller]
fn git_checkout(name: &str, work_dir: &Path) {
    let output = git_cmd(work_dir)
        .args(["checkout", name])
        .output()
        .expect("Failed to run git checkout");
    assert!(
        output.status.success(),
        "git checkout failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(any())]
#[allow(clippy::disallowed_methods)]
#[track_caller]
fn git_rev_parse(rev: &str, work_dir: &Path) -> String {
    let output = git_cmd(work_dir)
        .args(["rev-parse", rev])
        .output()
        .expect("Failed to run git rev-parse");
    assert!(
        output.status.success(),
        "git rev-parse failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

#[cfg(any())]
#[allow(clippy::disallowed_methods)]
#[track_caller]
fn git_cherry_pick_expect_conflict(commit: &str, work_dir: &Path) {
    let output = git_cmd(work_dir)
        .args(["cherry-pick", "--no-commit", commit])
        .output()
        .expect("Failed to run git cherry-pick");
    assert!(
        !output.status.success(),
        "git cherry-pick unexpectedly succeeded"
    );
}

#[cfg(any())]
#[allow(clippy::disallowed_methods)]
#[track_caller]
fn git_status(work_dir: &Path) -> collections::HashMap<String, String> {
    let output = git_cmd(work_dir)
        .args(["status", "--porcelain=v1"])
        .output()
        .expect("Failed to run git status");
    assert!(
        output.status.success(),
        "git status failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    stdout
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let status = line[..2].to_string();
            let path = line[3..].to_string();
            (path, status)
        })
        .collect()
}

#[gpui::test]
async fn test_find_project_path_abs(
    background_executor: BackgroundExecutor,
    cx: &mut gpui::TestAppContext,
) {
    // find_project_path should work with absolute paths
    init_test(cx);

    let fs = FakeFs::new(background_executor);
    fs.insert_tree(
        path!("/root"),
        json!({
            "project1": {
                "file1.txt": "content1",
                "subdir": {
                    "file2.txt": "content2"
                }
            },
            "project2": {
                "file3.txt": "content3"
            }
        }),
    )
    .await;

    let project = Project::test(
        fs.clone(),
        [
            path!("/root/project1").as_ref(),
            path!("/root/project2").as_ref(),
        ],
        cx,
    )
    .await;

    // Make sure the worktrees are fully initialized
    project
        .update(cx, |project, cx| project.git_scans_complete(cx))
        .await;
    cx.run_until_parked();

    let (project1_abs_path, project1_id, project2_abs_path, project2_id) =
        project.read_with(cx, |project, cx| {
            let worktrees: Vec<_> = project.worktrees(cx).collect();
            let abs_path1 = worktrees[0].read(cx).abs_path().to_path_buf();
            let id1 = worktrees[0].read(cx).id();
            let abs_path2 = worktrees[1].read(cx).abs_path().to_path_buf();
            let id2 = worktrees[1].read(cx).id();
            (abs_path1, id1, abs_path2, id2)
        });

    project.update(cx, |project, cx| {
        let abs_path = project1_abs_path.join("file1.txt");
        let found_path = project.find_project_path(abs_path, cx).unwrap();
        assert_eq!(found_path.worktree_id, project1_id);
        assert_eq!(&*found_path.path, rel_path("file1.txt"));

        let abs_path = project1_abs_path.join("subdir").join("file2.txt");
        let found_path = project.find_project_path(abs_path, cx).unwrap();
        assert_eq!(found_path.worktree_id, project1_id);
        assert_eq!(&*found_path.path, rel_path("subdir/file2.txt"));

        let abs_path = project2_abs_path.join("file3.txt");
        let found_path = project.find_project_path(abs_path, cx).unwrap();
        assert_eq!(found_path.worktree_id, project2_id);
        assert_eq!(&*found_path.path, rel_path("file3.txt"));

        let abs_path = project1_abs_path.join("nonexistent.txt");
        let found_path = project.find_project_path(abs_path, cx);
        assert!(
            found_path.is_some(),
            "Should find project path for nonexistent file in worktree"
        );

        // Test with an absolute path outside any worktree
        let abs_path = Path::new("/some/other/path");
        let found_path = project.find_project_path(abs_path, cx);
        assert!(
            found_path.is_none(),
            "Should not find project path for path outside any worktree"
        );
    });
}

#[gpui::test]
async fn test_git_worktree_remove(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/root"),
        json!({
            "a": {
                ".git": {},
                "src": {
                    "main.rs": "fn main() {}",
                }
            },
            "b": {
                ".git": {},
                "src": {
                    "main.rs": "fn main() {}",
                },
                "script": {
                    "run.sh": "#!/bin/bash"
                }
            }
        }),
    )
    .await;

    let project = Project::test(
        fs.clone(),
        [
            path!("/root/a").as_ref(),
            path!("/root/b/script").as_ref(),
            path!("/root/b").as_ref(),
        ],
        cx,
    )
    .await;
    let scan_complete = project.update(cx, |project, cx| project.git_scans_complete(cx));
    scan_complete.await;

    let worktrees = project.update(cx, |project, cx| project.worktrees(cx).collect::<Vec<_>>());
    assert_eq!(worktrees.len(), 3);

    let worktree_id_by_abs_path = worktrees
        .into_iter()
        .map(|worktree| worktree.read_with(cx, |w, _| (w.abs_path(), w.id())))
        .collect::<HashMap<_, _>>();
    let worktree_id = worktree_id_by_abs_path
        .get(Path::new(path!("/root/b/script")))
        .unwrap();

    let repos = project.update(cx, |p, cx| p.git_store().read(cx).repositories().clone());
    assert_eq!(repos.len(), 2);

    project.update(cx, |project, cx| {
        project.remove_worktree(*worktree_id, cx);
    });
    cx.run_until_parked();

    let mut repo_paths = project
        .update(cx, |p, cx| p.git_store().read(cx).repositories().clone())
        .values()
        .map(|repo| repo.read_with(cx, |r, _| r.work_directory_abs_path.clone()))
        .collect::<Vec<_>>();
    repo_paths.sort();

    pretty_assertions::assert_eq!(
        repo_paths,
        [
            Path::new(path!("/root/a")).into(),
            Path::new(path!("/root/b")).into(),
        ]
    );

    let active_repo_path = project
        .read_with(cx, |p, cx| {
            p.active_repository(cx)
                .map(|r| r.read(cx).work_directory_abs_path.clone())
        })
        .unwrap();
    assert_eq!(active_repo_path.as_ref(), Path::new(path!("/root/a")));

    let worktree_id = worktree_id_by_abs_path
        .get(Path::new(path!("/root/a")))
        .unwrap();
    project.update(cx, |project, cx| {
        project.remove_worktree(*worktree_id, cx);
    });
    cx.run_until_parked();

    let active_repo_path = project
        .read_with(cx, |p, cx| {
            p.active_repository(cx)
                .map(|r| r.read(cx).work_directory_abs_path.clone())
        })
        .unwrap();
    assert_eq!(active_repo_path.as_ref(), Path::new(path!("/root/b")));

    let worktree_id = worktree_id_by_abs_path
        .get(Path::new(path!("/root/b")))
        .unwrap();
    project.update(cx, |project, cx| {
        project.remove_worktree(*worktree_id, cx);
    });
    cx.run_until_parked();

    let active_repo_path = project.read_with(cx, |p, cx| {
        p.active_repository(cx)
            .map(|r| r.read(cx).work_directory_abs_path.clone())
    });
    assert!(active_repo_path.is_none());
}

#[gpui::test]
async fn test_optimistic_hunks_in_staged_files(cx: &mut gpui::TestAppContext) {
    use DiffHunkSecondaryStatus::*;
    init_test(cx);

    let committed_contents = r#"
        one
        two
        three
    "#
    .unindent();
    let file_contents = r#"
        one
        TWO
        three
    "#
    .unindent();

    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        path!("/dir"),
        json!({
            ".git": {},
            "file.txt": file_contents.clone()
        }),
    )
    .await;

    fs.set_head_and_index_for_repo(
        path!("/dir/.git").as_ref(),
        &[("file.txt", committed_contents.clone())],
    );

    let project = Project::test(fs.clone(), [path!("/dir").as_ref()], cx).await;

    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer(path!("/dir/file.txt"), cx)
        })
        .await
        .unwrap();
    let snapshot = buffer.read_with(cx, |buffer, _| buffer.snapshot());
    let uncommitted_diff = project
        .update(cx, |project, cx| {
            project.open_uncommitted_diff(buffer.clone(), cx)
        })
        .await
        .unwrap();

    // The hunk is initially unstaged.
    uncommitted_diff.read_with(cx, |diff, cx| {
        assert_hunks(
            diff.snapshot(cx).hunks(&snapshot),
            &snapshot,
            &diff.base_text_string(cx).unwrap(),
            &[(
                1..2,
                "two\n",
                "TWO\n",
                DiffHunkStatus::modified(HasSecondaryHunk),
            )],
        );
    });

    // Get the repository handle.
    let repo = project.read_with(cx, |project, cx| {
        project.repositories(cx).values().next().unwrap().clone()
    });

    // Stage the file.
    let stage_task = repo.update(cx, |repo, cx| {
        repo.stage_entries(vec![repo_path("file.txt")], cx)
    });

    // Run a few ticks to let the job start and mark hunks as pending,
    // but don't run_until_parked which would complete the entire operation.
    for _ in 0..10 {
        cx.executor().tick();
        let [hunk]: [_; 1] = uncommitted_diff
            .read_with(cx, |diff, cx| {
                diff.snapshot(cx).hunks(&snapshot).collect::<Vec<_>>()
            })
            .try_into()
            .unwrap();
        match hunk.secondary_status {
            HasSecondaryHunk => {}
            SecondaryHunkRemovalPending => break,
            NoSecondaryHunk => panic!("hunk was not optimistically staged"),
            _ => panic!("unexpected hunk state"),
        }
    }
    uncommitted_diff.read_with(cx, |diff, cx| {
        assert_hunks(
            diff.snapshot(cx).hunks(&snapshot),
            &snapshot,
            &diff.base_text_string(cx).unwrap(),
            &[(
                1..2,
                "two\n",
                "TWO\n",
                DiffHunkStatus::modified(SecondaryHunkRemovalPending),
            )],
        );
    });

    // Let the staging complete.
    stage_task.await.unwrap();
    cx.run_until_parked();

    // The hunk is now fully staged.
    uncommitted_diff.read_with(cx, |diff, cx| {
        assert_hunks(
            diff.snapshot(cx).hunks(&snapshot),
            &snapshot,
            &diff.base_text_string(cx).unwrap(),
            &[(
                1..2,
                "two\n",
                "TWO\n",
                DiffHunkStatus::modified(NoSecondaryHunk),
            )],
        );
    });

    // Simulate a commit by updating HEAD to match the current file contents.
    // The FakeGitRepository's commit method is a no-op, so we need to manually
    // update HEAD to simulate the commit completing.
    fs.set_head_for_repo(
        path!("/dir/.git").as_ref(),
        &[("file.txt", file_contents.clone())],
        "newhead",
    );
    cx.run_until_parked();

    // After committing, there are no more hunks.
    uncommitted_diff.read_with(cx, |diff, cx| {
        assert_hunks(
            diff.snapshot(cx).hunks(&snapshot),
            &snapshot,
            &diff.base_text_string(cx).unwrap(),
            &[] as &[(Range<u32>, &str, &str, DiffHunkStatus)],
        );
    });
}

#[gpui::test]
async fn test_read_only_files_setting(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    // Configure read_only_files setting
    cx.update(|cx| {
        cx.update_global::<SettingsStore, _>(|store, cx| {
            store.update_user_settings(cx, |settings| {
                settings.project.worktree.read_only_files = Some(vec![
                    "**/generated/**".to_string(),
                    "**/*.gen.rs".to_string(),
                ]);
            });
        });
    });

    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        path!("/root"),
        json!({
            "src": {
                "main.rs": "fn main() {}",
                "types.gen.rs": "// Generated file",
            },
            "generated": {
                "schema.rs": "// Auto-generated schema",
            }
        }),
    )
    .await;

    let project = Project::test(fs.clone(), [path!("/root").as_ref()], cx).await;

    // Open a regular file - should be read-write
    let regular_buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer(path!("/root/src/main.rs"), cx)
        })
        .await
        .unwrap();

    regular_buffer.read_with(cx, |buffer, _| {
        assert!(!buffer.read_only(), "Regular file should not be read-only");
    });

    // Open a file matching *.gen.rs pattern - should be read-only
    let gen_buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer(path!("/root/src/types.gen.rs"), cx)
        })
        .await
        .unwrap();

    gen_buffer.read_with(cx, |buffer, _| {
        assert!(
            buffer.read_only(),
            "File matching *.gen.rs pattern should be read-only"
        );
    });

    // Open a file in generated directory - should be read-only
    let generated_buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer(path!("/root/generated/schema.rs"), cx)
        })
        .await
        .unwrap();

    generated_buffer.read_with(cx, |buffer, _| {
        assert!(
            buffer.read_only(),
            "File in generated directory should be read-only"
        );
    });
}

#[gpui::test]
async fn test_read_only_files_empty_setting(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    // Explicitly set read_only_files to empty (default behavior)
    cx.update(|cx| {
        cx.update_global::<SettingsStore, _>(|store, cx| {
            store.update_user_settings(cx, |settings| {
                settings.project.worktree.read_only_files = Some(vec![]);
            });
        });
    });

    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        path!("/root"),
        json!({
            "src": {
                "main.rs": "fn main() {}",
            },
            "generated": {
                "schema.rs": "// Auto-generated schema",
            }
        }),
    )
    .await;

    let project = Project::test(fs.clone(), [path!("/root").as_ref()], cx).await;

    // All files should be read-write when read_only_files is empty
    let main_buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer(path!("/root/src/main.rs"), cx)
        })
        .await
        .unwrap();

    main_buffer.read_with(cx, |buffer, _| {
        assert!(
            !buffer.read_only(),
            "Files should not be read-only when read_only_files is empty"
        );
    });

    let generated_buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer(path!("/root/generated/schema.rs"), cx)
        })
        .await
        .unwrap();

    generated_buffer.read_with(cx, |buffer, _| {
        assert!(
            !buffer.read_only(),
            "Generated files should not be read-only when read_only_files is empty"
        );
    });
}

#[gpui::test]
#[cfg(not(windows))]
async fn test_os_read_only_files_open_as_read_only(cx: &mut gpui::TestAppContext) {
    init_test(cx);
    cx.executor().allow_parking();

    let root = TempTree::new(json!({
        "project": {
            "test.txt": "hello",
        },
    }));
    let file_path = root.path().join("project/test.txt");
    let mut permissions = std::fs::metadata(&file_path).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&file_path, permissions).unwrap();

    let project = Project::test(
        Arc::new(RealFs::new(None, cx.executor())),
        [root.path()],
        cx,
    )
    .await;

    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer(file_path.as_path(), cx)
        })
        .await
        .unwrap();

    buffer.read_with(cx, |buffer, _| {
        assert!(
            buffer.read_only(),
            "OS read-only files should open as read-only"
        );
    });
}

#[gpui::test]
async fn test_read_only_files_with_lock_files(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    // Configure to make lock files read-only
    cx.update(|cx| {
        cx.update_global::<SettingsStore, _>(|store, cx| {
            store.update_user_settings(cx, |settings| {
                settings.project.worktree.read_only_files = Some(vec![
                    "**/*.lock".to_string(),
                    "**/package-lock.json".to_string(),
                ]);
            });
        });
    });

    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        path!("/root"),
        json!({
            "Cargo.lock": "# Lock file",
            "Cargo.toml": "[package]",
            "package-lock.json": "{}",
            "package.json": "{}",
        }),
    )
    .await;

    let project = Project::test(fs.clone(), [path!("/root").as_ref()], cx).await;

    // Cargo.lock should be read-only
    let cargo_lock = project
        .update(cx, |project, cx| {
            project.open_local_buffer(path!("/root/Cargo.lock"), cx)
        })
        .await
        .unwrap();

    cargo_lock.read_with(cx, |buffer, _| {
        assert!(buffer.read_only(), "Cargo.lock should be read-only");
    });

    // Cargo.toml should be read-write
    let cargo_toml = project
        .update(cx, |project, cx| {
            project.open_local_buffer(path!("/root/Cargo.toml"), cx)
        })
        .await
        .unwrap();

    cargo_toml.read_with(cx, |buffer, _| {
        assert!(!buffer.read_only(), "Cargo.toml should not be read-only");
    });

    // package-lock.json should be read-only
    let package_lock = project
        .update(cx, |project, cx| {
            project.open_local_buffer(path!("/root/package-lock.json"), cx)
        })
        .await
        .unwrap();

    package_lock.read_with(cx, |buffer, _| {
        assert!(buffer.read_only(), "package-lock.json should be read-only");
    });

    // package.json should be read-write
    let package_json = project
        .update(cx, |project, cx| {
            project.open_local_buffer(path!("/root/package.json"), cx)
        })
        .await
        .unwrap();

    package_json.read_with(cx, |buffer, _| {
        assert!(!buffer.read_only(), "package.json should not be read-only");
    });
}

mod disable_ai_settings_tests {
    use gpui::TestAppContext;
    use project::*;
    use settings::{Settings, SettingsStore};

    #[gpui::test]
    async fn test_disable_ai_settings_security(cx: &mut TestAppContext) {
        cx.update(|cx| {
            settings::init(cx);

            // Test 1: Default is false (AI enabled)
            assert!(
                !DisableAiSettings::get_global(cx).disable_ai,
                "Default should allow AI"
            );
        });

        let disable_true = serde_json::json!({
            "disable_ai": true
        })
        .to_string();
        let disable_false = serde_json::json!({
            "disable_ai": false
        })
        .to_string();

        cx.update_global::<SettingsStore, _>(|store, cx| {
            store.set_user_settings(&disable_false, cx).unwrap();
            store.set_global_settings(&disable_true, cx).unwrap();
        });
        cx.update(|cx| {
            assert!(
                DisableAiSettings::get_global(cx).disable_ai,
                "Local false cannot override global true"
            );
        });

        cx.update_global::<SettingsStore, _>(|store, cx| {
            store.set_global_settings(&disable_false, cx).unwrap();
            store.set_user_settings(&disable_true, cx).unwrap();
        });

        cx.update(|cx| {
            assert!(
                DisableAiSettings::get_global(cx).disable_ai,
                "Local false cannot override global true"
            );
        });
    }

    #[gpui::test]
    async fn test_disable_ai_project_level_settings(cx: &mut TestAppContext) {
        use settings::{LocalSettingsKind, LocalSettingsPath, SettingsLocation, SettingsStore};
        use worktree::WorktreeId;

        cx.update(|cx| {
            settings::init(cx);

            // Default should allow AI
            assert!(
                !DisableAiSettings::get_global(cx).disable_ai,
                "Default should allow AI"
            );
        });

        let worktree_id = WorktreeId::from_usize(1);
        let rel_path = |path: &str| -> std::sync::Arc<util::rel_path::RelPath> {
            std::sync::Arc::from(util::rel_path::RelPath::from_unix_str(path).unwrap())
        };
        let project_path = rel_path("project");
        let settings_location = SettingsLocation {
            worktree_id,
            path: project_path.as_ref(),
        };

        // Test: Project-level disable_ai=true should disable AI for files in that project
        cx.update_global::<SettingsStore, _>(|store, cx| {
            store
                .set_local_settings(
                    worktree_id,
                    LocalSettingsPath::InWorktree(project_path.clone()),
                    LocalSettingsKind::Settings,
                    Some(r#"{ "disable_ai": true }"#),
                    cx,
                )
                .unwrap();
        });

        cx.update(|cx| {
            let settings = DisableAiSettings::get(Some(settings_location), cx);
            assert!(
                settings.disable_ai,
                "Project-level disable_ai=true should disable AI for files in that project"
            );
            // Global should now also be true since project-level disable_ai is merged into global
            assert!(
                DisableAiSettings::get_global(cx).disable_ai,
                "Global setting should be affected by project-level disable_ai=true"
            );
        });

        // Test: Setting project-level to false should allow AI for that project
        cx.update_global::<SettingsStore, _>(|store, cx| {
            store
                .set_local_settings(
                    worktree_id,
                    LocalSettingsPath::InWorktree(project_path.clone()),
                    LocalSettingsKind::Settings,
                    Some(r#"{ "disable_ai": false }"#),
                    cx,
                )
                .unwrap();
        });

        cx.update(|cx| {
            let settings = DisableAiSettings::get(Some(settings_location), cx);
            assert!(
                !settings.disable_ai,
                "Project-level disable_ai=false should allow AI"
            );
            // Global should also be false now
            assert!(
                !DisableAiSettings::get_global(cx).disable_ai,
                "Global setting should be false when project-level is false"
            );
        });

        // Test: User-level true + project-level false = AI disabled (saturation)
        let disable_true = serde_json::json!({ "disable_ai": true }).to_string();
        cx.update_global::<SettingsStore, _>(|store, cx| {
            store.set_user_settings(&disable_true, cx).unwrap();
            store
                .set_local_settings(
                    worktree_id,
                    LocalSettingsPath::InWorktree(project_path.clone()),
                    LocalSettingsKind::Settings,
                    Some(r#"{ "disable_ai": false }"#),
                    cx,
                )
                .unwrap();
        });

        cx.update(|cx| {
            let settings = DisableAiSettings::get(Some(settings_location), cx);
            assert!(
                settings.disable_ai,
                "Project-level false cannot override user-level true (SaturatingBool)"
            );
        });
    }
}

#[gpui::test]
async fn test_worktree_released_when_creation_caller_is_cancelled(cx: &mut gpui::TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(path!("/project"), serde_json::json!({ "a.rs": "" }))
        .await;
    fs.insert_tree(path!("/other"), serde_json::json!({ "b.rs": "" }))
        .await;
    let project = Project::test(fs, [path!("/project").as_ref()], cx).await;
    cx.run_until_parked();

    // Drop the returned task immediately, as a cancelled caller would; creation
    // still completes through the clone held by `loading_worktrees`.
    let task = project.update(cx, |project, cx| {
        project.find_or_create_worktree(path!("/other"), true, cx)
    });
    drop(task);
    cx.run_until_parked();

    let created = project.read_with(cx, |project, cx| {
        project
            .worktrees(cx)
            .find(|worktree| worktree.read(cx).abs_path().ends_with("other"))
            .map(|worktree| (worktree.read(cx).id(), worktree.downgrade()))
    });

    let Some((worktree_id, weak_worktree)) = created else {
        panic!("worktree creation should complete even when the caller is cancelled");
    };

    project.update(cx, |project, cx| project.remove_worktree(worktree_id, cx));
    cx.run_until_parked();

    assert!(
        weak_worktree.upgrade().is_none(),
        "removed worktree should be released even when its creation task's caller was cancelled"
    );
}

#[gpui::test]
async fn test_initial_scan_completes_when_creation_caller_is_cancelled(
    cx: &mut gpui::TestAppContext,
) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(path!("/project"), serde_json::json!({ "a.rs": "" }))
        .await;
    fs.insert_tree(path!("/other"), serde_json::json!({ "b.rs": "" }))
        .await;
    let project = Project::test(fs, [path!("/project").as_ref()], cx).await;
    cx.run_until_parked();

    let task = project.update(cx, |project, cx| {
        project.find_or_create_worktree(path!("/other"), true, cx)
    });
    drop(task);
    cx.run_until_parked();

    let (initial_scan_completed, loading_worktrees) = project.read_with(cx, |project, cx| {
        let worktree_store = project.worktree_store().read(cx);
        (
            worktree_store.initial_scan_completed(),
            worktree_store.diagnostics(cx).loading_worktrees,
        )
    });
    assert_eq!(
        loading_worktrees, 0,
        "no loading entry should remain after creation resolves"
    );
    assert!(
        initial_scan_completed,
        "initial scan should complete even when the creation task's caller was cancelled"
    );
}

/// Regression test for https://github.com/zed-industries/zed/issues/60424.
///
/// The committed text contains repeated `end\n\n` line runs, so the deletion
/// hunks have ambiguous placements. Before hunk placement was canonicalized,
/// the uncommitted diff (HEAD vs worktree) and the recomputed unstaged diff
/// (index vs worktree) could anchor the same logical deletion at different
/// rows after a partial stage. The mismatched hunk then rendered as staged
/// while git still had it unstaged, and unstaging it inserted a duplicate
/// copy of its contents into the index on every click.
#[gpui::test]
async fn test_staging_hunks_with_ambiguous_placement(cx: &mut gpui::TestAppContext) {
    init_test(cx);

    let committed_contents = concat!(
        "function is_empty(str)\n",
        "    assert(type(str) == string)\n",
        "    return str == nil or str == \"\"\n",
        "end\n",
        "\n",
        "function string:starts_with(str)\n",
        "    return self:sub(1, #str) == str\n",
        "end\n",
        "\n",
        "function string:ends_with(str)\n",
        "    return self:sub(-#str) == str\n",
        "end\n",
        "\n",
        "function table:append(other)\n",
        "    for _, val in ipairs(other) do table.insert(self, val) end\n",
        "end\n",
        "\n",
        "function table:contains(elem)\n",
        "    assert(type(self) == \"table\")\n",
        "\n",
        "    for _, val in ipairs(self) do\n",
        "        if val == elem then return true end\n",
        "    end\n",
        "    return false\n",
        "end\n",
        "\n",
        "function table:keys()\n",
        "    local keys = {}\n",
        "    for key, _ in pairs(self) do\n",
        "        table.insert(keys, key)\n",
        "    end\n",
        "    return keys\n",
        "end\n",
        "\n",
        "function has_class(el, class) return el.classes[0] == class end\n",
    )
    .to_string();
    let file_contents = concat!(
        "function table:append(other)\n",
        "    for _, val in ipairs(other) do table.insert(self, val) end\n",
        "end\n",
        "\n",
        "function table:keys()\n",
        "    local keys = {}\n",
        "    for key, _ in pairs(self) do\n",
        "        table.insert(keys, key)\n",
        "    end\n",
        "    return keys\n",
        "end\n",
        "\n",
        "function has_class(el, class) return el.classes:includes(class) end\n",
    )
    .to_string();

    let fs = FakeFs::new(cx.background_executor.clone());
    fs.insert_tree(
        "/dir",
        json!({
            ".git": {},
            "test.txt": file_contents.clone()
        }),
    )
    .await;
    fs.set_head_and_index_for_repo(
        path!("/dir/.git").as_ref(),
        &[("test.txt", committed_contents.clone())],
    );

    let project = Project::test(fs.clone(), ["/dir".as_ref()], cx).await;
    let buffer = project
        .update(cx, |project, cx| {
            project.open_local_buffer("/dir/test.txt", cx)
        })
        .await
        .unwrap();
    let snapshot = buffer.read_with(cx, |buffer, _| buffer.snapshot());
    let uncommitted_diff = project
        .update(cx, |project, cx| {
            project.open_uncommitted_diff(buffer.clone(), cx)
        })
        .await
        .unwrap();

    fn hunk_states(
        uncommitted_diff: &Entity<BufferDiff>,
        snapshot: &language::BufferSnapshot,
        cx: &mut gpui::TestAppContext,
    ) -> Vec<(Range<Point>, DiffHunkSecondaryStatus)> {
        uncommitted_diff.read_with(cx, |diff, cx| {
            diff.snapshot(cx)
                .hunks(&snapshot.text)
                .map(|hunk| (hunk.range.clone(), hunk.secondary_status))
                .collect()
        })
    }

    fn index_text(fs: &FakeFs) -> String {
        let bytes = fs
            .with_git_state(path!("/dir/.git").as_ref(), false, |state| {
                state
                    .index_contents
                    .get(&repo_path("test.txt"))
                    .cloned()
                    .expect("file is present in the index")
            })
            .unwrap();
        String::from_utf8(bytes).expect("test index contents are valid UTF-8")
    }

    use DiffHunkSecondaryStatus::*;
    let row = |start: u32, end: u32| Point::new(start, 0)..Point::new(end, 0);

    // Two deletions and one modification, all unstaged.
    assert_eq!(
        hunk_states(&uncommitted_diff, &snapshot, cx),
        vec![
            (row(0, 0), HasSecondaryHunk),
            (row(4, 4), HasSecondaryHunk),
            (row(12, 13), HasSecondaryHunk),
        ]
    );

    // Stage the first deletion (the three string functions).
    let hunks = uncommitted_diff.read_with(cx, |diff, cx| {
        diff.snapshot(cx).hunks(&snapshot.text).collect::<Vec<_>>()
    });
    project
        .update(cx, |project, cx| {
            let unstaged_diff = uncommitted_diff.read(cx).secondary_diff().unwrap();
            project.stage_hunks(
                buffer.clone(),
                unstaged_diff,
                vec![hunks[0].buffer_range.clone()],
                cx,
            )
        })
        .unwrap();
    cx.run_until_parked();

    // Only the first hunk is staged; the other hunks must not be affected,
    // even though the unstaged diff was recomputed against a new index text
    // in which the remaining deletion has an ambiguous placement.
    assert_eq!(
        hunk_states(&uncommitted_diff, &snapshot, cx),
        vec![
            (row(0, 0), NoSecondaryHunk),
            (row(4, 4), HasSecondaryHunk),
            (row(12, 13), HasSecondaryHunk),
        ]
    );
    let table_contains_offset = committed_contents
        .find("function table:append")
        .expect("committed text contains table:append");
    let expected_index = &committed_contents[table_contains_offset..];
    assert_eq!(index_text(&fs), expected_index);

    // Unstaging the second deletion is a no-op, since it isn't staged. Before
    // the fix, each such request inserted another copy of the deleted content
    // into the index.
    for _ in 0..3 {
        let hunks = uncommitted_diff.read_with(cx, |diff, cx| {
            diff.snapshot(cx).hunks(&snapshot.text).collect::<Vec<_>>()
        });
        project
            .update(cx, |project, cx| {
                project.unstage_uncommitted_hunks(
                    buffer.clone(),
                    uncommitted_diff.clone(),
                    vec![hunks[1].buffer_range.clone()],
                    cx,
                )
            })
            .unwrap();
        cx.run_until_parked();
    }
    assert_eq!(index_text(&fs), expected_index);
    assert_eq!(
        hunk_states(&uncommitted_diff, &snapshot, cx),
        vec![
            (row(0, 0), NoSecondaryHunk),
            (row(4, 4), HasSecondaryHunk),
            (row(12, 13), HasSecondaryHunk),
        ]
    );
}
