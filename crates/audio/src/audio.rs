use rodio::{ChannelCount, SampleRate, nz};

const SAMPLE_RATE: SampleRate = nz!(48000);
const CHANNEL_COUNT: ChannelCount = nz!(2);

mod audio_settings;
pub use audio_settings::AudioSettings;

mod audio_pipeline;
pub use audio_pipeline::Audio;
pub use audio_pipeline::init;

#[derive(Debug, Copy, Clone, Eq, Hash, PartialEq)]
pub enum Sound {
    AgentDone,
}

impl Sound {
    fn file(&self) -> &'static str {
        match self {
            Self::AgentDone => "agent_done",
        }
    }
}
