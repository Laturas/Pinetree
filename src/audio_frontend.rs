use rodio;
use std::{time, u128};

/* Exists because rodio is terrible */
use mp3_duration;


#[derive(PartialEq)]
pub enum LoopBehavior {
	Stop,
	Loop,
	Shuffle,
	Next,
}

#[derive(PartialEq)]
#[derive(Clone)]
#[derive(Copy)]
pub enum PrevBehavior {
	History,
	Above,
}

#[derive(Clone)]
pub struct RodioData {
	pub playback_position: usize,
	pub song_length: usize,
	pub is_paused: bool,
	pub song_name: String,
	pub error_message: Option<String>,
	pub shuffle_memory: usize,
}

pub struct PlaylistTreeElement {
	// None = it's a playlist
	pub song_name: Option<String>,
	pub playlist_position: usize,
}

pub fn request_rodio_data(send_pair: &std::sync::Arc<(std::sync::Mutex<Vec<MessageToAudio>>, std::sync::Condvar)>, recv_pair: &std::sync::Arc<(std::sync::Mutex<Vec<RodioData>>, std::sync::Condvar)>) -> RodioData {
	let lock = &recv_pair.0;
	let cvar = &recv_pair.1;
	send_audio_signal(send_pair, MessageToAudio::RequestRodioData);

	if let Ok(mut vec) = lock.lock() {
		loop {
			while vec.len() == 0 {
				vec = cvar.wait(vec).unwrap();
			}
			if let Some(element) = vec.pop() {
				return element;
			}
		}
	}
	return RodioData {
		playback_position: 0,
		song_length: 0,
		song_name: "".to_string(),
		is_paused: false,
		error_message: None,
		shuffle_memory: 0,
	};
	// vec = if let Ok(mut vec) 
}

pub fn send_audio_signal(pair: &std::sync::Arc<(std::sync::Mutex<Vec<MessageToAudio>>, std::sync::Condvar)>, message: MessageToAudio) {
	let lock = &pair.0;
	let cvar = &pair.1;
	if let Ok(mut data) = lock.lock() {
		data.push(message);
		cvar.notify_one();
	}
}

#[derive(PartialEq)]
pub enum MessageToAudio {
	// None,
	PlaySong(String),
	UpdateEndBehavior(LoopBehavior),
	UpdateVolume(f32),
	UpdateSpeed(f32),
	RequestRodioData,
	Seek(usize),
	SeekStop,
	TogglePause,
	SongEnd,
	/**
	* Tells the audio thread to set its current collection to Vec<String>,
	* and that the current playing song is at the optional location
	*/
	SetSongCollection(Vec<String>, Option<usize>),
	ClearError,
	PreviousSong,
	UpdateShuffleMemory(usize),
	UpdatePrevBehavior(PrevBehavior),
}

struct EndCallback {
    callback: Option<Box<dyn FnOnce() + Send>>,
}

impl Iterator for EndCallback {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
		if let Some(cb) = self.callback.take() {
            cb();
        }
        None
    }
}

impl Source for EndCallback {
    fn current_frame_len(&self) -> Option<usize> { None }
    fn channels(&self) -> u16 { 2 }
    fn sample_rate(&self) -> u32 { 44100 }
    fn total_duration(&self) -> Option<Duration> { Some(Duration::ZERO) }
}

struct AudioThreadData {
	// This has to exist even if unused, otherwise the lifetime causes the program to crash
	_stream: rodio::OutputStream,
	sink: rodio::Sink,
	volume: f32,
	speed: f32,
	end_behavior: LoopBehavior,
}

use rodio::Source;
use std::time::{Duration, SystemTime};

/**
 * Defaults to 0
 */
pub fn get_song_len_ms(file_path: &str) -> usize {
	if let Ok(len) = mp3_duration::from_path(&file_path) {
		return len.as_millis() as usize;
	}
	return 0;
}

pub fn audio_thread_play_song(file_path: &str, sink: &mut rodio::Sink, recieve_pair: &std::sync::Arc<(std::sync::Mutex<Vec<MessageToAudio>>, std::sync::Condvar)>) -> Option<String> {
	let mut return_value = None;
	if let Ok(file) = std::fs::File::open(&file_path) {
		let reader = std::io::BufReader::<std::fs::File>::new(file);
		
		if let Ok(elem) = rodio::Decoder::new_mp3(reader) {
			sink.clear();
			let _ = sink.try_seek(std::time::Duration::from_millis(0));
			
			sink.append(elem);

			let rodio_pair = std::sync::Arc::clone(recieve_pair);
			sink.append(EndCallback {
				callback: Some(Box::new(move || {
					song_end_callback(rodio_pair);
				})),
			});
		} else {
			return_value = Some(format!("Error: Invalid mp3 format {}", file_path));
		}
	} else {
		return_value = Some(format!("Error: failed to open mp3 file {}", file_path));
	}
	sink.play();
	return return_value;
}

pub const DEFAULT_VOLUME: f32 = 0.75;
pub const DEFAULT_SPEED: f32 = 1.0;

pub fn clone_loop_behavior(behavior: &LoopBehavior) -> LoopBehavior {
	return match *behavior {
		LoopBehavior::Stop => LoopBehavior::Stop,
		LoopBehavior::Loop => LoopBehavior::Loop,
		LoopBehavior::Shuffle => LoopBehavior::Shuffle,
		LoopBehavior::Next => LoopBehavior::Next,
	}
}

pub fn song_end_callback(pair: std::sync::Arc<(std::sync::Mutex<Vec<MessageToAudio>>, std::sync::Condvar)>) {
	send_audio_signal(&pair, MessageToAudio::SongEnd);
}

pub fn generate_random_number(random_seed: u128) -> u128 {
	let aff = random_seed.wrapping_mul(928594379).wrapping_add(531881627);
	// This XOR operation is necessary because the low bits have low entropy.
	// The higher bits are highly random though, so mixing them in with the lower bits gives more randomness
	let xor_1 = aff ^ (aff >> 12);
	let xor_2 = xor_1 ^ (xor_1 >> 23);
	let xor_3 = xor_2 ^ (xor_2 >> 50);
	let xor_4 = xor_3 ^ (xor_3 >> 73);
	let xor_5 = xor_4 ^ (xor_4 >> 97);

	return xor_5;
}

pub fn initialize_random_seed() -> u128 {
	let start = std::time::SystemTime::now();
	let since_the_epoch = start
		.duration_since(std::time::UNIX_EPOCH)
		.expect("time should go forward");
	let mut random_seed: u128 = since_the_epoch.as_micros();

	for _ in 0..50 {
		random_seed = generate_random_number(random_seed);
	}

	return random_seed;
}

struct SongRingBuffer {
	vec: Vec<String>,
	front: usize,
	back: usize,
	current_element: usize,
}

fn new_ring_buffer(capacity: usize) -> SongRingBuffer {
	let mut v = Vec::<String>::new();
	for _ in 0..capacity {
		v.push("".to_string());
	}
	return SongRingBuffer {
		vec: v,
		front: 0,
		back: 0,
		current_element: 0,
	};
}

/*
******XXXXXX*******
      ^Back ^Front
*/
fn push_to_ring_buffer(buffer: &mut SongRingBuffer, element: &str) {
	if ((buffer.front + 1) % buffer.vec.len()) == buffer.back {
		buffer.back = (buffer.back + 1) % buffer.vec.len();
	}
	buffer.vec[buffer.front] = element.to_string();
	buffer.front = (buffer.front + 1) % buffer.vec.len();
}

/**
* Returns true if it successfully goes back to the previous song
*/
fn try_go_to_previous_song(buffer: &mut SongRingBuffer) -> bool {
	if buffer.current_element == buffer.back {
		return false;
	} else {
		let cap = buffer.vec.len();
		buffer.current_element = (buffer.current_element + cap - 1) % cap;
		return true;
	}
}
fn _try_go_to_next_song(buffer: &mut SongRingBuffer) -> bool {
	let cap = buffer.vec.capacity();
	let max_el = ((buffer.front + cap) - 1) % cap;
	if buffer.current_element == max_el {
		return false;
	} else {
		buffer.current_element = (buffer.current_element + 1) % cap;
		return true;
	}
}

fn song_is_in_last_n(song: &String, history_buffer: &SongRingBuffer, count: usize) -> bool {
	let mut found = false;

	let mut cur = history_buffer.front;
	let cap = history_buffer.vec.capacity();
	/* Just helps make sure we don't divide by zero */
	if cap == 0 {
		return false;
	}

	for _ in 0..count {
		if let Some(s) = history_buffer.vec.get(cur) {
			if s == song {
				found = true;
				break;
			}
		}
		
		if cur == history_buffer.back {
			break;
		}
		cur = (cur + cap - 1) % cap;
	}

	return found;
}

fn play_song(try_save_to_history: bool,
	song: &str,
	current_song: &mut String,
	history_buffer: &mut SongRingBuffer,
	audio_thread_data: &mut AudioThreadData,
	recieve_pair: &std::sync::Arc<(std::sync::Mutex<Vec<MessageToAudio>>, std::sync::Condvar)>) -> Option<String>
{
	if try_save_to_history && song != *current_song {
		push_to_ring_buffer(history_buffer, &song);
		history_buffer.current_element = ((history_buffer.front + history_buffer.vec.capacity()) - 1) % history_buffer.vec.capacity();
	}

	{ /* Song playing */
		let err = audio_thread_play_song(&song, &mut audio_thread_data.sink, recieve_pair);
		if err.is_none() {
			*current_song = song.to_string();
		}
		return err;
	}
}
pub fn update_timestamps(song: &str,
	song_length: &mut usize,
	current_timestamp: &mut u128,
	saved_timestamp: &mut Option<SystemTime>)
{
	*song_length = get_song_len_ms(song);
	*current_timestamp = 0;
	*saved_timestamp = Some(time::SystemTime::now());
}


/**
* Human hearing is logarithmic, so the volume slider follows an exponential curve to compensate.
*
* Specifically this is one that is meant to very closely match the decibel scale (hence the magic number of 6.908).
* Has to be clamped though unfortunately, because exponential curves never technically reach 0.
*
* This creates a sharp cutoff at the lowest volume, but at that point it's quiet enough I don't imagine people will notice.
*
* Surprised this isn't more common.
*/
pub fn volume_curve(input: f32) -> f32 {
	if input <= -0.195 {return 0.0;}
	return (input * 6.908).exp() / 1000.0
}

/**
* Audio thread maintains its own list it works on, going through each element in that during the "Next".
* 
* When a new song is played:
* - GUI thread checks if the current directory tree is the same as the one the audio thread has
* 	- If it isn't, send an updated one.
* - Send the current song.
* - Audio thread looks through to find the new index.
*/
pub fn audio_thread_loop(
	recieve_pair: std::sync::Arc<(std::sync::Mutex<Vec<MessageToAudio>>, std::sync::Condvar)>,
	send_pair: std::sync::Arc<(std::sync::Mutex<Vec<RodioData>>, std::sync::Condvar)>
) {
	let (output_stream, audio_sink) = rodio::OutputStream::try_default().unwrap();
	
	let mut random_seed = initialize_random_seed();

	let mut song_path = "".to_string();
	let mut song_index = 0;
	let mut song_length = 0;
	let mut audio_thread_data = AudioThreadData {
		// This has to exist even if unused, otherwise the lifetime causes the program to crash
		_stream: output_stream,
		sink: rodio::Sink::try_new(&audio_sink).unwrap(),
		volume: volume_curve(DEFAULT_VOLUME),
		speed: DEFAULT_SPEED,
		end_behavior: LoopBehavior::Stop,
	};
	audio_thread_data.sink.set_volume(audio_thread_data.volume);
	let lock = &recieve_pair.0;
	let cvar = &recieve_pair.1;
	let mut data_vec = lock.lock().unwrap();
	let mut current_songs_collection = Vec::<String>::new();
	let mut saved_timestamp: Option<time::SystemTime> = None;
	let mut current_timestamp: u128 = 0;
	let mut song_play_err = None;
	let mut seeking = false;
	let mut paused_from_seeking = false;
	let mut randomization_memory = 5;
	let mut prev_behavior: PrevBehavior = PrevBehavior::Above;

	let mut history_buffer = new_ring_buffer(255);

	loop {
		while let Some(data) = data_vec.pop() {
			match data {
				// MessageToAudio::None => {println!("Do nothing");},
				MessageToAudio::PlaySong(song) => {
					song_play_err = play_song(true, &song, &mut song_path, &mut history_buffer, &mut audio_thread_data, &recieve_pair);
					if song_play_err.is_none() {
						update_timestamps(&song_path, &mut song_length, &mut current_timestamp, &mut saved_timestamp);

						/* TODO: Avoid having to do this O(n) loop */
						song_index = 0;
						for i in 0..current_songs_collection.len() {
							if let Some(e) = current_songs_collection.get(i) && *e == song_path {
								song_index = i;
								break;
							}
						}
					}
				},
				MessageToAudio::UpdateEndBehavior(loop_behavior) => {
					audio_thread_data.end_behavior = clone_loop_behavior(&loop_behavior);
				},
				MessageToAudio::UpdateVolume(volume) => {
					audio_thread_data.volume = volume;
					audio_thread_data.sink.set_volume(audio_thread_data.volume);
				},
				MessageToAudio::UpdateSpeed(speed) => {
					if let Some(ts) = saved_timestamp && let Ok(a) = ts.elapsed() {
						current_timestamp += (a.as_micros() as f32 * audio_thread_data.speed) as u128;
						saved_timestamp = Some(time::SystemTime::now());
					}
					audio_thread_data.speed = speed;
					audio_thread_data.sink.set_speed(speed);
				},
				MessageToAudio::RequestRodioData => {
					if let Some(ts) = saved_timestamp && let Ok(a) = ts.elapsed() {
						current_timestamp += (a.as_micros() as f32 * audio_thread_data.speed) as u128;
						saved_timestamp = Some(time::SystemTime::now());
					}

					let send_lock = &send_pair.0;
					let send_cvar = &send_pair.1;
					let mut response_vec = send_lock.lock().unwrap();
					response_vec.push(RodioData {
						song_length: song_length,
						playback_position: (current_timestamp / 1000) as usize,
						is_paused: audio_thread_data.sink.is_paused(),
						song_name: song_path.clone(),
						error_message: song_play_err.clone(),
						shuffle_memory: randomization_memory,
					});
					send_cvar.notify_one();
				},
				MessageToAudio::Seek(position) => {
					seeking = true;
					if !audio_thread_data.sink.empty() {
						let seek_time_ms: u64 = (position as f32 / audio_thread_data.speed) as u64;
						if (song_length as u64).saturating_sub(seek_time_ms) < 1000 {
							if !audio_thread_data.sink.is_paused() && !paused_from_seeking {
								paused_from_seeking = true;
							}
							audio_thread_data.sink.pause();
						} else {
							if audio_thread_data.sink.is_paused() && paused_from_seeking {
								audio_thread_data.sink.play();
								paused_from_seeking = false;
							}
						}
						let _ = audio_thread_data.sink.try_seek(std::time::Duration::from_millis(seek_time_ms));
						current_timestamp = (position * 1000) as u128;
						saved_timestamp = if audio_thread_data.sink.is_paused() {None} else {Some(time::SystemTime::now())};
					}
				},
				MessageToAudio::SeekStop => {
					if seeking && !audio_thread_data.sink.empty() {
						if audio_thread_data.sink.is_paused() && paused_from_seeking {
							audio_thread_data.sink.play();
							paused_from_seeking = false;
						}
						seeking = false;
						saved_timestamp = if audio_thread_data.sink.is_paused() {None} else {Some(time::SystemTime::now())};
					}
				},
				MessageToAudio::TogglePause => {
					if audio_thread_data.sink.is_paused() {
						saved_timestamp = Some(time::SystemTime::now());
						audio_thread_data.sink.play();
					} else {
						if let Some(ts) = saved_timestamp && let Ok(a) = ts.elapsed() {
							current_timestamp += (a.as_micros() as f32 * audio_thread_data.speed) as u128;
							saved_timestamp = None;
						}
						audio_thread_data.sink.pause();
					}
				},
				MessageToAudio::SongEnd => {
					audio_thread_data.sink.clear();
					match audio_thread_data.end_behavior {
						LoopBehavior::Stop => {
							song_path = "".to_string();

							current_timestamp = 0;
							saved_timestamp = None;
						},
						LoopBehavior::Loop => {
							let path_clone = song_path.clone(); /* Borrow checker agony */
							let err = play_song(false, &path_clone, &mut song_path, &mut history_buffer, &mut audio_thread_data, &recieve_pair);
							if err.is_none() {
								update_timestamps(&song_path, &mut song_length, &mut current_timestamp, &mut saved_timestamp);
							}
						},
						LoopBehavior::Next => {
							let mut next_song: Option<String> = None;
							let mut push_to_history = false;
							if current_songs_collection.len() > 0 {
								song_index = (song_index + 1) % current_songs_collection.len() as usize;

								if let Some(song) = current_songs_collection.get(song_index) {
									push_to_history = true;
									next_song = Some(song.clone());
								}
							}
							if let Some(song) = next_song {
								song_play_err = play_song(push_to_history, &song, &mut song_path, &mut history_buffer, &mut audio_thread_data, &recieve_pair);
								if song_play_err.is_none() {
									update_timestamps(&song_path, &mut song_length, &mut current_timestamp, &mut saved_timestamp);
								}
							} else {
								song_length = 1;
								song_path = "".to_string();
								current_timestamp = 0;
								saved_timestamp = None;
							}
						}
						LoopBehavior::Shuffle => {
							if current_songs_collection.len() > 0 {
								let mut song_chosen = false;
								for _ in 0..32 {
									random_seed = generate_random_number(random_seed);
									let song_index = ((random_seed % (usize::MAX as u128)) as usize) % current_songs_collection.len();
									if let Some(song) = current_songs_collection.get(song_index) {
										if song_is_in_last_n(song, &history_buffer, randomization_memory) {
											continue;
										}
										song_play_err = play_song(true, &song, &mut song_path, &mut history_buffer, &mut audio_thread_data, &recieve_pair);
										if song_play_err.is_none() {
											update_timestamps(&song_path, &mut song_length, &mut current_timestamp, &mut saved_timestamp);
										}
										song_chosen = true;
										break;
									}
								}
								if !song_chosen {
									random_seed = generate_random_number(random_seed);
									let song_index = ((random_seed % (usize::MAX as u128)) as usize) % current_songs_collection.len();
									if let Some(song) = current_songs_collection.get(song_index) {
										song_play_err = play_song(true, &song, &mut song_path, &mut history_buffer, &mut audio_thread_data, &recieve_pair);
										if song_play_err.is_none() {
											update_timestamps(&song_path, &mut song_length, &mut current_timestamp, &mut saved_timestamp);
										}
									}
								}
							} else {
								song_length = 1;
								song_path = "".to_string();
								current_timestamp = 0;
								saved_timestamp = None;
							}
						}
					}
				},
				MessageToAudio::SetSongCollection(vec, optional_index) => {
					current_songs_collection = vec;
					song_index = if let Some(index) = optional_index {index}
						else {current_songs_collection.len().saturating_sub(1)};
				},
				MessageToAudio::ClearError => {
					song_play_err = None;
				},
				MessageToAudio::PreviousSong => {
					match prev_behavior {
						PrevBehavior::Above => {
							let mut prev_song: Option<String> = None;
							let mut push_to_history = false;
							if current_songs_collection.len() > 0 {
								song_index = (song_index + current_songs_collection.len() - 1) % current_songs_collection.len() as usize;

								if let Some(song) = current_songs_collection.get(song_index) {
									push_to_history = true;
									prev_song = Some(song.clone());
								}
							}
							if let Some(song) = prev_song {
								song_play_err = play_song(push_to_history, &song, &mut song_path, &mut history_buffer, &mut audio_thread_data, &recieve_pair);
								if song_play_err.is_none() {
									update_timestamps(&song_path, &mut song_length, &mut current_timestamp, &mut saved_timestamp);
								}
							} else {
								song_length = 1;
								song_path = "".to_string();
								current_timestamp = 0;
								saved_timestamp = None;
							}
						},
						PrevBehavior::History => {
							if try_go_to_previous_song(&mut history_buffer) {
								let song = history_buffer.vec[history_buffer.current_element].clone();
								song_play_err = play_song(false, &song, &mut song_path, &mut history_buffer, &mut audio_thread_data, &recieve_pair);
								if song_play_err.is_none() {
									update_timestamps(&song_path, &mut song_length, &mut current_timestamp, &mut saved_timestamp);
									song_index = 0;
									for i in 0..current_songs_collection.len() {
										if let Some(e) = current_songs_collection.get(i) && *e == song {
											song_index = i;
											break;
										}
									}
								}
							}
						},
					}
					
				},
				MessageToAudio::UpdateShuffleMemory(mem) => {
					randomization_memory = mem;
				},
				MessageToAudio::UpdatePrevBehavior(new_behavior) => {
					prev_behavior = new_behavior;
				}
			}
		}
		// If this unwrap fails, it should crash.
		data_vec = cvar.wait(data_vec).unwrap();
	}
}