use crate::audio_frontend::*;

pub enum MessageToEngine {
	PlaySong(String),
	UpdateVolume(f32),
	UpdateSpeed(f32),
	Seek(usize),
	TogglePause,
}

pub fn engine_loop(
	receive_pair: std::sync::Arc<(std::sync::Mutex<Vec<MessageToEngine>>, std::sync::Condvar)>,
	send_pair: std::sync::Arc<(std::sync::Mutex<Vec<MessageToAudio>>, std::sync::Condvar)>
) {
	let lock = &receive_pair.0;
	let cvar = &receive_pair.1;
	let mut data_vec = lock.lock().unwrap();

	loop {
		while let Some(message) = data_vec.pop() {
			match message {
				
			}
		}
	}
}