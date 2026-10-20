mod edit_prediction_button;

use command_palette_hooks::CommandPaletteFilter;
use project::DisableAiSettings;
use settings::{Settings as _, SettingsStore};
use std::any::TypeId;
use ui::App;

pub use edit_prediction_button::{
    EditPredictionButton, ToggleMenu, get_available_providers, set_completion_provider,
};

pub fn init(cx: &mut App) {
    let action_types = [TypeId::of::<edit_prediction::ClearHistory>()];

    cx.observe_global::<SettingsStore>(move |cx| {
        let is_ai_disabled = DisableAiSettings::get_global(cx).disable_ai;

        CommandPaletteFilter::update_global(cx, |filter, _cx| {
            if is_ai_disabled {
                filter.hide_action_types(&action_types);
            } else {
                filter.show_action_types(&action_types);
            }
        });
    })
    .detach();
}
