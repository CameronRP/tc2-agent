use crate::{RecordingModeState, RecordingState};

pub struct AgentService {
    pub recording_mode_state: RecordingModeState,
    pub recording_state: RecordingState,
}

impl AgentService {
    pub fn is_taking_test_audio_recording(&self) -> bool {
        self.recording_state.is_taking_test_audio_recording()
    }
}
