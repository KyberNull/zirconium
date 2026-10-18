use collections::HashMap;
use gpui::{App, Entity, Task};
use language::{Buffer, ContextProvider, LocalFile as _};
use std::str::FromStr;
use task::{TaskTemplate, TaskTemplates, VariableName};
use util::rel_path::RelPath;

use crate::PackageJsonData;

pub(crate) struct JsonTaskProvider;

impl ContextProvider for JsonTaskProvider {
    fn associated_tasks(
        &self,
        buffer: Option<Entity<Buffer>>,
        cx: &App,
    ) -> gpui::Task<Option<TaskTemplates>> {
        let file = buffer.as_ref().and_then(|buf| buf.read(cx).file());
        let Some(file) = project::File::from_dyn(file).cloned() else {
            return Task::ready(None);
        };
        let is_package_json = file
            .path
            .ends_with(RelPath::from_unix_str("package.json").unwrap());
        let is_composer_json = file
            .path
            .ends_with(RelPath::from_unix_str("composer.json").unwrap());
        if !is_package_json && !is_composer_json {
            return Task::ready(None);
        }

        cx.spawn(async move |cx| {
            let contents = file
                .worktree
                .update(cx, |this, cx| this.load_file(&file.path, cx))
                .await
                .ok()?;
            let path = cx.update(|cx| file.abs_path(cx)).as_path().into();
            let contents_text = contents.text.to_string();

            let task_templates = if is_package_json {
                let package_json = serde_json_lenient::from_str::<
                    HashMap<String, serde_json_lenient::Value>,
                >(&contents_text)
                .ok()?;
                let package_json = PackageJsonData::new(path, package_json);
                let command = package_json.package_manager.unwrap_or("npm").to_owned();
                package_json
                    .scripts
                    .into_iter()
                    .map(|(_, key)| TaskTemplate {
                        label: format!("run {key}"),
                        command: command.clone(),
                        args: vec!["run".into(), key],
                        cwd: Some(VariableName::Dirname.template_value()),
                        ..TaskTemplate::default()
                    })
                    .chain([TaskTemplate {
                        label: "package script $ZED_CUSTOM_script".to_owned(),
                        command: command.clone(),
                        args: vec![
                            "run".into(),
                            VariableName::Custom("script".into()).template_value(),
                        ],
                        cwd: Some(VariableName::Dirname.template_value()),
                        tags: vec!["package-script".into()],
                        ..TaskTemplate::default()
                    }])
                    .collect()
            } else if is_composer_json {
                serde_json_lenient::Value::from_str(&contents_text)
                    .ok()?
                    .get("scripts")?
                    .as_object()?
                    .keys()
                    .map(|key| TaskTemplate {
                        label: format!("run {key}"),
                        command: "composer".to_owned(),
                        args: vec!["-d".into(), "$ZED_DIRNAME".into(), key.into()],
                        ..TaskTemplate::default()
                    })
                    .chain([TaskTemplate {
                        label: "composer script $ZED_CUSTOM_script".to_owned(),
                        command: "composer".to_owned(),
                        args: vec![
                            "-d".into(),
                            "$ZED_DIRNAME".into(),
                            VariableName::Custom("script".into()).template_value(),
                        ],
                        tags: vec!["composer-script".into()],
                        ..TaskTemplate::default()
                    }])
                    .collect()
            } else {
                vec![]
            };

            Some(TaskTemplates(task_templates))
        })
    }
}
