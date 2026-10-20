use anyhow::Result;
use serde_json::Value;

use crate::migrations::migrate_settings;

pub fn remove_zed_edit_prediction_provider(value: &mut Value) -> Result<()> {
    migrate_settings(value, &mut migrate_one)
}

fn migrate_one(obj: &mut serde_json::Map<String, Value>) -> Result<()> {
    if let Some(edit_predictions) = obj
        .get_mut("edit_predictions")
        .and_then(|edit_predictions| edit_predictions.as_object_mut())
    {
        replace_zed_provider(edit_predictions, "provider");
        edit_predictions.remove("zed");
        edit_predictions.remove("allow_data_collection");
    }

    if let Some(features) = obj
        .get_mut("features")
        .and_then(|features| features.as_object_mut())
    {
        replace_zed_provider(features, "edit_prediction_provider");
    }

    Ok(())
}

fn replace_zed_provider(obj: &mut serde_json::Map<String, Value>, field_name: &str) {
    if obj.get(field_name).and_then(Value::as_str) == Some("zed") {
        obj.insert(field_name.to_string(), Value::String("none".to_string()));
    }
}
