#![windows_subsystem = "windows"]

/**
 * Pinetree MP3 Player Version 2 Internal Source Code
 * 
 * by Katelyn Doucette
 */

use std::sync::{Arc, Mutex, Condvar};
use std::{thread, time, u128};
use std::collections::HashMap;
use std::io::BufRead;
use std::panic;

use eframe::egui;

use egui::{InputState, Key};
use rodio;

/* Exists because rodio is terrible */
use mp3_duration;

#[derive(PartialEq)]
enum LoopBehavior {
	Stop,
	Loop,
	Shuffle,
	Next,
}

#[derive(PartialEq)]
#[derive(Clone)]
#[derive(Copy)]
enum PrevBehavior {
	History,
	Above,
}

#[derive(PartialEq)]
#[derive(Clone)]
enum LeftPanelMode {
	Files,
	Playlists,
	DeletePlaylist,
	SelectSongs,
	RemoveSongs,
	ReorderSongs,
}

/**
* Contains all of the serialized data of a song
*/
struct Song {
	filepath_identifier: String,
	name: String,
	genre: String,
	artist: String,
	time_listened_ms: u64,
	// playlists: Vec<String>
}
#[derive(Debug)]
struct Directory {
	filepath_identifier: String,
	subdirectories: Vec<String>,
	songs: Vec<String>
}

#[derive(PartialEq)]
enum FileActions {
	None,
	OpenDirectory(String),

	OpenDirectoryRecursive(String),
	
	CloseDirectory(String),
	EnterDirectory(String),
	PlaySong(String),

	OpenPlaylist(usize),
	ClosePlaylist(usize),
	EnterPlaylist(usize),
	AddSongToPlaylist(usize),
	AddSongToPlaylistStrong(usize),
}

struct Playlist {
	name: String,
	songs: Vec<String>,
	is_open: bool,
}

#[derive(PartialEq)]
#[derive(Clone)]
enum ThemePref {
	DARK,
	LIGHT,
}

fn theme_to_str(theme: &ThemePref) -> String {
	return match theme {
		ThemePref::DARK => "Dark".to_string(),
		ThemePref::LIGHT => "Light".to_string(),
	};
}

#[derive(PartialEq)]
enum CentralPanelMode {
	Installer,
	InstallationSuccess,
	Settings,
	PlayerMode,
	About,
}

pub const CURRENT_VERSION: &str = "OPEN BETA 5";

struct PersistentData {
	data_file_exists: bool,
	hide_directories_by_default: bool,
	data_file_version: String,
	default_directory: String,
	theme: ThemePref,
	playlists: Vec<Playlist>,
	default_on_finish: LoopBehavior,
	default_volume: f32,
	shuffle_memory: usize,
	prev_behavior: PrevBehavior,
}

fn default_persistent_data() -> PersistentData {
	PersistentData {
		data_file_exists: false,
		hide_directories_by_default: false,
		data_file_version: CURRENT_VERSION.to_string(),
		default_directory: "".to_string(),
		theme: ThemePref::DARK,
		playlists: init_playlist_from_filepath(""),
		default_on_finish: LoopBehavior::Stop,
		default_volume: DEFAULT_VOLUME,
		shuffle_memory: 3,
		prev_behavior: PrevBehavior::History,
	}
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

struct InstallerData {
	// create_taskbar_shortcut: bool,
	// create_desktop_shortcut: bool,
	// run_from_terminal: bool,

	install_path: String,
	default_song_folder: String,
}

/**
 * This code was mostly lifted from the dirs-sys with changes
 * https://github.com/dirs-dev/dirs-sys-rs/
 * 
 * It felt irresponsible adding a dependency for this when the code is simple,
 * and I don't need the entirety of the dependency
 */
#[cfg(target_os = "windows")]
mod windows_folders {
	use std::ffi::OsString;
	use std::os::windows::ffi::OsStringExt;
	use windows_sys::Win32::UI::Shell;

	pub fn try_get_winfolder(folder_id: windows_sys::core::GUID) -> Option<std::path::PathBuf> {
		unsafe {
			let mut path_ptr: windows_sys::core::PWSTR = std::ptr::null_mut();
			let result = Shell::SHGetKnownFolderPath(&folder_id, 0,std::ptr::null_mut(),&mut path_ptr);
			if result == 0 {
				let len = windows_sys::Win32::Globalization::lstrlenW(path_ptr) as usize;
				let path = std::slice::from_raw_parts(path_ptr, len);
				let ostr: OsString = OsStringExt::from_wide(path);
				windows_sys::Win32::System::Com::CoTaskMemFree(path_ptr as *const std::ffi::c_void);
				Some(std::path::PathBuf::from(ostr))
			} else {
				windows_sys::Win32::System::Com::CoTaskMemFree(path_ptr as *const std::ffi::c_void);
				None
			}
		}
	}

	pub fn roaming_app_data() -> Option<std::path::PathBuf> {
		try_get_winfolder(Shell::FOLDERID_RoamingAppData)
	}

	// pub fn local_app_data() -> Option<std::path::PathBuf> {
	// 	try_get_winfolder(Shell::FOLDERID_LocalAppData)
	// }

	pub fn music() -> Option<std::path::PathBuf> {
		try_get_winfolder(Shell::FOLDERID_Music)
	}

	// pub fn desktop() -> Option<std::path::PathBuf> {
	// 	try_get_winfolder(Shell::FOLDERID_Desktop)
	// }
}

#[cfg(target_family = "unix")]
mod unix_folders {
	use std::env;

	/**
	* Returns ~/
	*/
	pub fn home() -> Option<String> {
		let mut r_str = None;
		if let Some(home) = env::home_dir() && let Some(path_string) = home.as_os_str().to_str() {
			r_str = Some(path_string.to_string());
		}
		return r_str;
	}
}

fn default_install_path() -> String {
	#[cfg(target_os = "windows")] {
		let appdata = windows_folders::roaming_app_data();
		if let Some(appdata_folder) = appdata
		&& let Some(path_string) = appdata_folder.as_os_str().to_str() {
			return path_string.to_string();
		}
	}
	#[cfg(target_os = "linux")] {
		let home = unix_folders::home();
		if let Some(home_folder) = home {
			let local_share = build_full_filepath(&home_folder, ".local/share");
			return local_share.to_string();
		}
	}
	#[cfg(target_os = "macos")] {
		let home = unix_folders::home();
		if let Some(home_folder) = home {
			let local_share = build_full_filepath(&home_folder, "Library/Application Support");
			return local_share.to_string();
		}
	}
	"".to_string()
}
fn default_song_path() -> String {
	#[cfg(target_os = "windows")] {
		let appdata = windows_folders::music();
		if let Some(appdata_folder) = appdata
		&& let Some(path_string) = appdata_folder.as_os_str().to_str() {
			return path_string.to_string();
		}
	}
	#[cfg(target_os = "linux")] {
		let home = unix_folders::home();
		if let Some(home_folder) = home {
			let local_share = build_full_filepath(&home_folder, "Music");
			return local_share.to_string();
		}
	}
	#[cfg(target_os = "macos")] {
		let home = unix_folders::home();
		if let Some(home_folder) = home {
			let local_share = build_full_filepath(&home_folder, "Music");
			return local_share.to_string();
		}
	}
	"".to_string()
}

fn default_installer_data() -> InstallerData {
	return InstallerData {
		// create_taskbar_shortcut: false,
		// create_desktop_shortcut: false,
		// run_from_terminal: false,

		install_path: default_install_path(),
		default_song_folder: default_song_path(),
	};
}

struct MyApp {
	// For setting sizing and such
	first_frame_rendered: bool,

	loop_behavior: LoopBehavior,
	default_on_finish: LoopBehavior,
	browse_mode: LeftPanelMode,

	current_song_folder: String,

	search_text: String,
	advanced_search_active: bool,
	genre_search_text: String,
	artist_search_text: String,
	
	song_speed: f32,
	song_volume: f32,

	active_directory_filepath: String,

	directory_map: HashMap<String, Directory>,
	song_map: HashMap<String, Song>,

	directory_tree: Option<Vec<DirTreeElement>>,
	playlist_tree: Option<Vec<PlaylistTreeElement>>,

	active_search_text: String,
	searched_directory_tree: Option<Vec<usize>>,

	active_playlist_index: Option<usize>,

	active_search_text_playlists: String, 
	searched_playlist_tree: Option<Vec<usize>>,

	edit_playlist_data: Option<PlaylistEditData>,

	audio_message_channel: Arc<(Mutex<Vec<MessageToAudio>>, Condvar)>,
	audio_receive_channel: Arc<(Mutex<Vec<MessageToGui>>, Condvar)>,

	central_panel_mode: CentralPanelMode,

	persistent_data: PersistentData,
	installer_data: InstallerData,
	installed_location: String,
	installer_error: Option<String>,

	hide_fp: bool,

	save_err: SaveError,

	pinned_mode: bool,
	shuffle_memory: usize,
	shuffle_memory_text: String,

	prev_behavior: PrevBehavior,
}

#[derive(Clone)]
struct RodioData {
	playback_position: usize,
	song_length: usize,
	is_paused: bool,
	song_name: String,
	error_message: Option<String>,
	shuffle_memory: usize,
}

enum MessageToGui {
	// None,
	Data(RodioData),
}

struct PlaylistTreeElement {
	// None = it's a playlist
	song_name: Option<String>,
	playlist_position: usize,
}

struct AudioThreadData {
	// This has to exist even if unused, otherwise the lifetime causes the program to crash
	_stream: rodio::OutputStream,
	sink: rodio::Sink,
	volume: f32,
	speed: f32,
	end_behavior: LoopBehavior,
}

fn build_playlist_tree(playlists: &Vec<Playlist>) -> Vec<PlaylistTreeElement> {
	let mut vec: Vec<PlaylistTreeElement> = Vec::<PlaylistTreeElement>::new();
	let mut i = 0;
	for playlist in playlists {
		vec.push(PlaylistTreeElement {
			song_name: None,
			playlist_position: i,
		});
		if playlist.is_open {
			for song in &playlist.songs {
				vec.push(PlaylistTreeElement {
					song_name: Some(song.clone()),
					playlist_position: 0,
				});
			}
		}
		i += 1;
	}
	return vec;
}

/**
* The playlist data takes the following form. We can assume that playlist names and file paths don't
* contain newlines even if they're technically legal on unix systems because egui wouldn't be able to render it anyways.
* That is the only character type we can assume is illegal though.
*
* Playlist: playlist_name
* song_filepath
* song_filepath
* song_filepath
* ...
* Playlist: playlist_2_name
* ...
*/
fn init_playlist_from_filepath(fp: &str) -> Vec<Playlist> {
	let file = std::fs::File::open(fp);
	let mut playlists = Vec::<Playlist>::new();
	let mut current_songs = Vec::<String>::new();
	let mut playlist_name: Option<String> = None;

	if let Ok(file) = file {
		let reader = std::io::BufReader::new(file);
		for line in reader.lines() {
			if let Ok(line) = line {
				// Drop the current playlist (add to vec)
				if line.starts_with("Playlist: ") {
					if let Some(name) = playlist_name {
						playlists.push(Playlist {
							name: name,
							songs: current_songs,
							is_open: false,
						});
						current_songs = Vec::<String>::new();
					}
					playlist_name = Some((&line[10..]).to_string());
				} else {
					current_songs.push(line);
				}
			}
		}
	}
	if let Some(name) = playlist_name {
		playlists.push(Playlist {
			name: name,
			songs: current_songs,
			is_open: false,
		});
	}
	return playlists;
}
use rodio::Source;
use std::time::{Duration, SystemTime};

/**
 * Defaults to 0
 */
fn get_song_len_ms(file_path: &str) -> usize {
	if let Ok(len) = mp3_duration::from_path(&file_path) {
		return len.as_millis() as usize;
	}
	return 0;
}

fn audio_thread_play_song(file_path: &str, sink: &mut rodio::Sink, recieve_pair: &Arc<(Mutex<Vec<MessageToAudio>>, Condvar)>) -> Option<String> {
	let mut return_value = None;
	if let Ok(file) = std::fs::File::open(&file_path) {
		let reader = std::io::BufReader::<std::fs::File>::new(file);
		
		if let Ok(elem) = rodio::Decoder::new_mp3(reader) {
			sink.clear();
			let _ = sink.try_seek(std::time::Duration::from_millis(0));
			
			sink.append(elem);

			let rodio_pair = Arc::clone(recieve_pair);
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

fn clone_loop_behavior(behavior: &LoopBehavior) -> LoopBehavior {
	return match *behavior {
		LoopBehavior::Stop => LoopBehavior::Stop,
		LoopBehavior::Loop => LoopBehavior::Loop,
		LoopBehavior::Shuffle => LoopBehavior::Shuffle,
		LoopBehavior::Next => LoopBehavior::Next,
	}
}

fn song_end_callback(pair: Arc<(Mutex<Vec<MessageToAudio>>, Condvar)>) {
	send_audio_signal(&pair, MessageToAudio::SongEnd);
}

fn generate_random_number(random_seed: u128) -> u128 {
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

fn initialize_random_seed() -> u128 {
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
fn try_go_to_next_song(buffer: &mut SongRingBuffer) -> bool {
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
	recieve_pair: &Arc<(Mutex<Vec<MessageToAudio>>, Condvar)>) -> Option<String>
{
	let mut err = None;
	if try_save_to_history && song != *current_song {
		push_to_ring_buffer(history_buffer, &song);
		history_buffer.current_element = ((history_buffer.front + history_buffer.vec.capacity()) - 1) % history_buffer.vec.capacity();
	}

	{ /* Song playing */
		err = audio_thread_play_song(&song, &mut audio_thread_data.sink, recieve_pair);
		if err.is_none() {
			*current_song = song.to_string();
		}
	}
	return err;
}
fn update_timestamps(song: &str,
	song_length: &mut usize,
	current_timestamp: &mut u128,
	saved_timestamp: &mut Option<SystemTime>)
{
	*song_length = get_song_len_ms(song);
	*current_timestamp = 0;
	*saved_timestamp = Some(time::SystemTime::now());
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
fn audio_thread_loop(
	recieve_pair: Arc<(Mutex<Vec<MessageToAudio>>, Condvar)>,
	send_pair: Arc<(Mutex<Vec<MessageToGui>>, Condvar)>
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
					response_vec.push(MessageToGui::Data(RodioData {
						song_length: song_length,
						playback_position: (current_timestamp / 1000) as usize,
						is_paused: audio_thread_data.sink.is_paused(),
						song_name: song_path.clone(),
						error_message: song_play_err.clone(),
						shuffle_memory: randomization_memory,
					}));
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

fn request_rodio_data(send_pair: &Arc<(Mutex<Vec<MessageToAudio>>, Condvar)>, recv_pair: &Arc<(Mutex<Vec<MessageToGui>>, Condvar)>) -> RodioData {
	let lock = &recv_pair.0;
	let cvar = &recv_pair.1;
	send_audio_signal(send_pair, MessageToAudio::RequestRodioData);

	if let Ok(mut vec) = lock.lock() {
		loop {
			while vec.len() == 0 {
				vec = cvar.wait(vec).unwrap();
			}
			if let Some(element) = vec.pop() {
				match element {
					// MessageToGui::None => {
					// 	vec.pop();
					// },
					MessageToGui::Data(data) => {
						let ret = data.clone();
						return ret;
					},
				}
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

fn send_audio_signal(pair: &Arc<(Mutex<Vec<MessageToAudio>>, Condvar)>, message: MessageToAudio) {
	let lock = &pair.0;
	let cvar = &pair.1;
	if let Ok(mut data) = lock.lock() {
		data.push(message);
		cvar.notify_one();
	}
}

#[derive(PartialEq)]
enum MessageToAudio {
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

fn str_to_theme_preference(string: &str) -> ThemePref {
	return match string {
		"Dark" => ThemePref::DARK,
		"Light" => ThemePref::LIGHT,

		/* Default is dark */
		&_ => ThemePref::DARK,
	};
}

fn find_persistent_data() -> (PersistentData, String) {
	let mut ret_str = "".to_string();
	let data_file = if let Ok(same_dir) = std::fs::File::open("internal_pinetree_data.txt") {
		ret_str = "./".to_string();
		same_dir
	} else {
		let pinetree_folder = &build_full_filepath(&default_install_path(), "Pinetree");
		let internal_data_file = build_full_filepath(pinetree_folder, "internal_pinetree_data.txt");

		if let Ok(default_install_location) = std::fs::File::open(internal_data_file) {
			ret_str = build_full_filepath(&default_install_path(), "Pinetree");
			default_install_location
		} else {
			return (default_persistent_data(), ret_str);
		}
	} ;
	let mut persistent_data = default_persistent_data();

	persistent_data.data_file_exists = true;
	let reader = std::io::BufReader::new(data_file);

	let mut current_songs = Vec::<String>::new();
	let mut playlist_name: Option<String> = None;

	#[derive(PartialEq)]
	enum State {
		Version,
		Settings,
		Playlists,
	}

	let mut current_state = State::Version;
	
	for line in reader.lines() {
		if let Ok(line) = line {
			let version_header = "VERSION: ";
			let settings_header = "SETTINGS";
			let playlists_header = "PLAYLISTS";

			if line.starts_with(version_header) {
				persistent_data.data_file_version = line[version_header.len()..].to_string();
			}
			else if line.starts_with(settings_header) {
				current_state = State::Settings;
			}
			else if line.starts_with(playlists_header) {
				current_state = State::Playlists;
			}
			else if current_state == State::Settings {
				let theme_identifier = "Theme: ";
				let default_directory_identifier = "Default Directory: ";
				let hide_dirs_identifier = "Hide Directories: "; 
				let default_end_behavior_identifier = "Default End Behavior: "; 
				let default_volume_identifier = "Default Volume: ";
				let shuffle_memory_identifier = "Shuffle Memory: ";
				let prev_behavior_identifier= "Default Prev Behavior: ";
				if line.starts_with(theme_identifier) {
					persistent_data.theme = str_to_theme_preference(&line[theme_identifier.len()..]);
				} else if line.starts_with(default_directory_identifier) {
					persistent_data.default_directory = (line[default_directory_identifier.len()..]).to_string();
				} else if line.starts_with(hide_dirs_identifier) {
					match &line[hide_dirs_identifier.len()..] {
						"true" => {
							persistent_data.hide_directories_by_default = true;
						},
						"false" => {
							persistent_data.hide_directories_by_default = false;
						},
						_ => {},
					}
				} else if line.starts_with(default_end_behavior_identifier) {
					match &line[default_end_behavior_identifier.len()..] {
						"Loop" => {
							persistent_data.default_on_finish = LoopBehavior::Loop;
						},
						"Stop" => {
							persistent_data.default_on_finish = LoopBehavior::Stop;
						},
						"Shuffle" => {
							persistent_data.default_on_finish = LoopBehavior::Shuffle;
						},
						"Next" => {
							persistent_data.default_on_finish = LoopBehavior::Next;
						},
						_ => {},
					}
				} else if line.starts_with(default_volume_identifier) {
					persistent_data.default_volume = line[default_volume_identifier.len()..].parse().unwrap_or(DEFAULT_VOLUME);
				} else if line.starts_with(shuffle_memory_identifier) {
					persistent_data.shuffle_memory = line[shuffle_memory_identifier.len()..].parse().unwrap_or(3);
				} else if line.starts_with(prev_behavior_identifier) {
					match &line[prev_behavior_identifier.len()..] {
						"Above" => {
							persistent_data.prev_behavior = PrevBehavior::Above;
						},
						"History" => {
							persistent_data.prev_behavior = PrevBehavior::History;
						},
						_ => {},
					}
				}
			}
			else if current_state == State::Playlists {
				/* Prevents trailing newlines from causing problems */
				if line.len() < 1 {continue;}

				if line.starts_with("Playlist: ") {
					if let Some(name) = playlist_name {
						persistent_data.playlists.push(Playlist {
							name: name,
							songs: current_songs,
							is_open: false,
						});
						current_songs = Vec::<String>::new();
					}
					playlist_name = Some((&line[10..]).to_string());
				} else {
					current_songs.push(line);
				}
			}
		}
	}
	/* Playlists are currently the last thing in the internal data */
	if let Some(name) = playlist_name {
		persistent_data.playlists.push(Playlist {
			name: name,
			songs: current_songs,
			is_open: false,
		});
	}

	return (persistent_data, ret_str);
}
use std::io::Write;
fn initialize_crash_logger(pinetree_directory: &str) {
	let log_location = build_full_filepath(pinetree_directory, "crash_report.log").to_string();
	panic::set_hook(Box::new(move |panic_info| {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_location)
            .unwrap();

        let msg = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            *s
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.as_str()
        } else {
            "Unknown panic payload"
        };

        let location = if let Some(loc) = panic_info.location() {
            format!("{}:{}", loc.file(), loc.line())
        } else {
            "Unknown location".to_string()
        };

		let backtrace = std::backtrace::Backtrace::force_capture();

        let _ = writeln!(file, "Panic occurred: {}\nLocation: {}\nBacktrace: {:?}", msg, location, backtrace);
    }));
}

impl Default for MyApp {
	fn default() -> Self {
		let message_param: Vec<MessageToAudio> = Vec::<MessageToAudio>::new();
		let gui_message_param: Vec<MessageToGui> = Vec::<MessageToGui>::new();
		
		let audio_thread_recieve = Arc::new((Mutex::new(message_param), Condvar::new()));
		let gui_thread_send = Arc::clone(&audio_thread_recieve);
		
		let audio_thread_send = Arc::new((Mutex::new(gui_message_param), Condvar::new()));
		let gui_thread_recieve = Arc::clone(&audio_thread_send);
		
		thread::spawn(move || {
			audio_thread_loop(audio_thread_recieve, audio_thread_send);
		});
		
		let (persistent_data, installed_location) = find_persistent_data();

		let mut dir_map = HashMap::<String, Directory>::new();
		init_directory_at_filepath(&persistent_data.default_directory, &mut dir_map);

		let start_mode = if persistent_data.data_file_exists {
			if persistent_data.data_file_version != CURRENT_VERSION {
				println!("Updating executable");
				let target_exe_path = build_full_filepath(&installed_location, "pinetree.exe");
	
				if let Ok(current_exe) = std::env::current_exe() {
					if let Ok(_) = std::fs::copy(&current_exe, &target_exe_path) {
						// Nothing: Success
						// Continue as normal
					}
				}
			}
			CentralPanelMode::Settings
		} else {
			CentralPanelMode::Installer
		};

		let crash_log_location = if persistent_data.data_file_exists {
			&installed_location
		} else {
			&"./".to_string()
		};

		initialize_crash_logger(crash_log_location);

		Self {
			first_frame_rendered: false,
			loop_behavior: clone_loop_behavior(&persistent_data.default_on_finish),
			default_on_finish: clone_loop_behavior(&persistent_data.default_on_finish),
			browse_mode: LeftPanelMode::Files,
			current_song_folder: persistent_data.default_directory.clone(),
			song_speed: DEFAULT_SPEED,
			search_text: "".to_string(),
			genre_search_text: "".to_string(),
			artist_search_text: "".to_string(),
			advanced_search_active: false,
			song_volume: persistent_data.default_volume,
			// songs_list: song_entry_list,
			active_directory_filepath: persistent_data.default_directory.clone(),
			directory_map: dir_map,
			song_map: HashMap::<String, Song>::new(),
			directory_tree: None,
			playlist_tree: None,

			active_search_text: "".to_string(),
			searched_directory_tree: None,
			searched_playlist_tree: None,
			
			active_playlist_index: None,
			
			active_search_text_playlists: "".to_string(),

			edit_playlist_data: None,

			audio_message_channel: gui_thread_send,
			audio_receive_channel: gui_thread_recieve,
			hide_fp: persistent_data.hide_directories_by_default.clone(),
			shuffle_memory: persistent_data.shuffle_memory,
			shuffle_memory_text: format!("{}", persistent_data.shuffle_memory),
			prev_behavior: persistent_data.prev_behavior,

			persistent_data: persistent_data,

			central_panel_mode: start_mode,

			installer_data: default_installer_data(),
			installed_location: installed_location,

			installer_error: None,

			save_err: SaveError::None,

			pinned_mode: false,
		}
	}
}

fn trim_slash_if_exists(to_trim: &str) -> &str {
	if to_trim.ends_with('/') || to_trim.ends_with('\\') {
		return &to_trim[..to_trim.len() - 1];
	} else {
		return &to_trim;
	}
}

fn build_full_filepath(input_first_half: &str, input_second_half: &str) -> String {
	let mergeable_first_half = trim_slash_if_exists(input_first_half);
	#[cfg(target_family = "windows")]
	return format!("{}\\{}", mergeable_first_half, input_second_half);
	#[cfg(target_family = "unix")]
	return format!("{}/{}", mergeable_first_half, input_second_half);
}

fn init_directory_at_filepath(directory_filepath: &str, dir_map: &mut HashMap<String, Directory>) -> bool {
	let read_result = std::fs::read_dir(directory_filepath);
	if let Ok(paths) = read_result {
		let mut songs_vec = Vec::<String>::new();
		let mut subdirectory_vec = Vec::<String>::new();

		for path in paths {
			if let Ok(valid_path) = path {
				if let Ok(file_name) = valid_path.file_name().into_string() {

					if let Ok(file_type) = valid_path.file_type() && file_type.is_dir() {
						subdirectory_vec.push(build_full_filepath(directory_filepath, &file_name));
					} else if file_name.ends_with(".mp3") {
						songs_vec.push(build_full_filepath(directory_filepath, &file_name));
					}

				}
			}
		}
		/* Is there a more efficient way? This makes copies */
		songs_vec.sort_by_key(|name| name.to_lowercase());
		subdirectory_vec.sort_by_key(|name| name.to_lowercase());

		dir_map.insert(directory_filepath.to_string(), Directory {filepath_identifier: directory_filepath.to_string(), subdirectories: subdirectory_vec, songs: songs_vec});
		return true;
	} else {
		return false;
	}
}

fn init_directory_at_filepath_recursive(directory_filepath: &str, dir_map: &mut HashMap<String, Directory>) -> bool {
	let read_result = std::fs::read_dir(directory_filepath);
	if let Ok(paths) = read_result {
		let mut songs_vec = Vec::<String>::new();
		let mut subdirectory_vec = Vec::<String>::new();
		for path in paths {
			if let Ok(valid_path) = path {
				if let Ok(file_name) = valid_path.file_name().into_string() {
					if let Ok(file_type) = valid_path.file_type() && file_type.is_dir() {
						subdirectory_vec.push(build_full_filepath(directory_filepath, &file_name));
					} else if file_name.ends_with(".mp3") {
						songs_vec.push(build_full_filepath(directory_filepath, &file_name));
					} 
				}
			}
		}
		/* Is there a more efficient way? This makes copies */
		songs_vec.sort_by_key(|name| name.to_lowercase());
		subdirectory_vec.sort_by_key(|name| name.to_lowercase());

		for directory in &subdirectory_vec {
			init_directory_at_filepath_recursive(directory, dir_map);
		}

		dir_map.insert(directory_filepath.to_string(), Directory {filepath_identifier: directory_filepath.to_string(), subdirectories: subdirectory_vec, songs: songs_vec});

		return true;
	} else {
		return false;
	}
}


fn loop_behavior_to_str(lb: &LoopBehavior) -> &'static str {
	match lb {
		LoopBehavior::Stop => "Stop",
		LoopBehavior::Loop => "Loop",
		LoopBehavior::Shuffle => "Shuffle",
		LoopBehavior::Next => "Next",
	}
}

fn song_browse_mode_to_str(song_browse_mode: &LeftPanelMode) -> &'static str {
	match song_browse_mode {
		LeftPanelMode::Files => "Files",
		LeftPanelMode::Playlists => "Playlists",
		LeftPanelMode::DeletePlaylist => "Delete",
		LeftPanelMode::RemoveSongs => "Remove", 
		LeftPanelMode::SelectSongs => "Select", 
		LeftPanelMode::ReorderSongs => "Reorder",
	}
}

/**
* Backs up from e.g. ~/Music/ to ~/
*/
fn song_folder_go_up(input_string: &str) -> String {
	let last_char = {
		// This is here because otherwise with an extra slash it will back up to the same folder and just delete the slash.
		let operated_str = trim_slash_if_exists(input_string);
		let last_bslash = operated_str.rfind('\\');
		let last_slash = operated_str.rfind('/');

		match (last_bslash, last_slash) {
			(Some(bslash), Some(slash)) => {
				if bslash > slash {bslash} else {slash}
			}
			(Some(bslash), None) => bslash,
			(None, Some(slash)) => slash,
			(None, None) => 0,
		}
	};
	format!("{}", if last_char == 0 {""} else {&input_string[0..last_char]})
}

fn extract_file_name(input_string: &str) -> &str {
	let last_bslash = input_string.rfind('\\');
	let last_slash = input_string.rfind('/');

	let start = match (last_bslash, last_slash) {
		(Some(bslash), Some(slash)) => {
			if bslash > slash {bslash} else {slash}
		}
		(Some(bslash), None) => bslash,
		(None, Some(slash)) => slash,
		(None, None) => 0,
	};
	return if start == 0 {input_string} else {&input_string[start + 1..]};
}

fn extract_folder_name(input_string: &str) -> &str {
	let input_string = if input_string.ends_with('\\') {
		&input_string[..input_string.len() - 1]
	} else {
		input_string
	};
	let last_bslash = input_string.rfind('\\');
	let last_slash = input_string.rfind('/');

	let start = match (last_bslash, last_slash) {
		(Some(bslash), Some(slash)) => {
			if bslash > slash {bslash} else {slash}
		}
		(Some(bslash), None) => bslash,
		(None, Some(slash)) => slash,
		(None, None) => 0,
	};
	return if start == 0 {input_string} else {&input_string[start + 1..]};
}

fn render_song_entry_ui_element(ui: &mut egui::Ui, current_song: &str, is_current_song: bool, depth: usize, edit_playlist_data: &Option<PlaylistEditData>) -> FileActions {
	let mut return_value = FileActions::None;
	ui.horizontal(|ui| {
		ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);

		/* Indenting */
		for _ in 0..depth {ui.add_space(10.0);}

		if let Some(playlist_data) = edit_playlist_data {
			let mut b = playlist_data.edit_map.contains_key(current_song);
			if ui.checkbox(&mut b, "").clicked() {
				let shift_pressed = ui.input(|i| i.modifiers.shift);

				if shift_pressed {
					return_value = FileActions::AddSongToPlaylistStrong(0);
				} else {
					return_value = FileActions::AddSongToPlaylist(0);
				}
			}
		} else {
			if ui.button("▶").clicked() {
				return_value = FileActions::PlaySong(current_song.to_string());
			}
		}

		if is_current_song {
			ui.label(egui::RichText::new(extract_file_name(current_song)).underline().strong());
		}
		else {
			ui.label(extract_file_name(current_song));
		}
	});
	return return_value;
}

fn render_directory_entry_ui_element(ui: &mut egui::Ui, current_directory: &str, is_expanded: bool, depth: usize) -> FileActions {
	let mut return_value = FileActions::None;
	ui.horizontal(|ui| {
		ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
		for _ in 0..depth {
			ui.add_space(10.0);
		}
		if is_expanded {
			if ui.button("−").clicked() {
				return_value = FileActions::CloseDirectory(current_directory.to_string());
			}
		} else {
			if ui.button("+").clicked() {
				let shift_pressed = ui.input(|i| i.modifiers.shift);
				if shift_pressed {
					return_value = FileActions::OpenDirectoryRecursive(current_directory.to_string());
				} else {
					return_value = FileActions::OpenDirectory(current_directory.to_string());
				}
			}
		}
		if ui.add(egui::Link::new(extract_file_name(current_directory))).on_hover_text_at_pointer("Enter this folder").clicked() {
			return_value = FileActions::EnterDirectory(current_directory.to_string());
		}
	});
	return return_value;
}

struct DirTreeElement {
	name: String,
	depth: usize,
	is_dir: bool,
	/* For directories only, indicating whether it is active or not */
	is_active: bool,
}

fn get_dir_tree_elements(output_vec: &mut Vec<DirTreeElement>, directory_string: &str, map: &HashMap<String, Directory>, depth: usize) {
	let map_result = map.get(directory_string);
	if let Some(directory) = map_result {
		for subdirectory_string in &directory.subdirectories {
			let is_active = match map.get(subdirectory_string) {Some(_) => true, None => false};
			output_vec.push(DirTreeElement {
				name: subdirectory_string.to_string(),
				depth: depth,
				is_dir: true,
				is_active: is_active
			});
			if is_active {get_dir_tree_elements(output_vec, subdirectory_string, map, depth + 1)};
		}

		for song_string in &directory.songs {
			output_vec.push(DirTreeElement {
				name: song_string.to_string(),
				depth: depth,
				is_dir: false,
				is_active: false,
			});
		}
	}
}

pub const REFRESH: egui::KeyboardShortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::R);
pub const PAUSE: egui::KeyboardShortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::P);
pub const PREV_SONG: egui::KeyboardShortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::ArrowLeft);
pub const NEXT_SONG: egui::KeyboardShortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::ArrowRight);

fn render_directory_elements(
	ui: &mut egui::Ui,
	directory_tree_vec: &Option<Vec<DirTreeElement>>,
	searched_vec: &Option<Vec<usize>>,
	active_song_name: &str,
	edit_playlist_data: &Option<PlaylistEditData>) -> FileActions {
	let mut file_action = FileActions::None;
	if let Some(directory_tree_elements) = directory_tree_vec {
		let row_count = if let Some(s) = searched_vec {s.len()} else {directory_tree_elements.len()};
		egui::ScrollArea::vertical().show_rows(ui, 16.0, row_count, |ui, row_range| {
			ui.set_min_width(ui.available_rect_before_wrap().size().x);
	
			for row in row_range {
				let get_element = if let Some(search_vec) = searched_vec {
					if let Some(index) = search_vec.get(row) {
						*index
					} else {
						continue;
					}
				} else {
					row
				};
				if let Some(element) = directory_tree_elements.get(get_element) {
					let re_code = if element.is_dir {
						render_directory_entry_ui_element(ui, &element.name, element.is_active, element.depth)
					} else {
						let is_active_song = &element.name == active_song_name;
						render_song_entry_ui_element(ui, &element.name, is_active_song, element.depth, edit_playlist_data)
					};
					match re_code {
						FileActions::None => {},
						FileActions::OpenDirectory(dir) => {
							file_action = FileActions::OpenDirectory(dir.clone());
						},
						FileActions::OpenDirectoryRecursive(dir) => {
							file_action = FileActions::OpenDirectoryRecursive(dir.clone());
						},
						FileActions::CloseDirectory(dir) => {
							file_action = FileActions::CloseDirectory(dir.clone());
						},
						FileActions::EnterDirectory(dir) => {
							file_action = FileActions::EnterDirectory(dir.clone());
						},
						FileActions::PlaySong(song) => {
							file_action = FileActions::PlaySong(song.clone());
						},
						FileActions::AddSongToPlaylist(_) => {
							file_action = FileActions::AddSongToPlaylist(get_element);
						},
						FileActions::AddSongToPlaylistStrong(_) => {
							file_action = FileActions::AddSongToPlaylistStrong(get_element);
						},
						_ => {},
					}
				}
			}
		});
	}
	return file_action;
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
fn volume_curve(input: f32) -> f32 {
	if input <= -0.195 {return 0.0;}
	return (input * 6.908).exp() / 1000.0
}

/**
* playlist_tree_vec: This is the vector containing the elements that can be rendered.
* searched_vec: This is an optional vec with indices into the playlist_tree_vec for specific elements that should be rendered yielded by searching.
* playlists: This is the original playlists vector containing data like the playlist name. The tree vec doesn't store that data to prevent duplication.
* song_depth: If a playlist is open we render songs at depth 0. If we're looking at all playlists, they render at depth 1.
*/
fn render_playlist_elements(ui: &mut egui::Ui,
	playlist_tree_vec: &Option<Vec<PlaylistTreeElement>>,
	searched_vec: &Option<Vec<usize>>,
	playlists: &Vec<Playlist>,
	song_depth: usize,
	active_song_name: &str) -> FileActions
{
	let mut file_action = FileActions::None;
	if let Some(playlist_tree_elements) = playlist_tree_vec {
		let row_count = if let Some(s) = searched_vec {s.len()} else {playlist_tree_elements.len()};
		egui::ScrollArea::vertical().show_rows(ui, 16.0, row_count, |ui, row_range| {
			ui.set_min_width(ui.available_rect_before_wrap().size().x);
	
			for row in row_range {
				let get_element = if let Some(search_vec) = searched_vec {
					if let Some(index) = search_vec.get(row) {
						*index
					} else {
						continue;
					}
				} else {
					row
				};
				if let Some(element) = playlist_tree_elements.get(get_element) {
					if let Some(song_name) = &element.song_name {
						let is_active_song = song_name == active_song_name;
						if let FileActions::PlaySong(song) = render_song_entry_ui_element(ui, &song_name, is_active_song, song_depth, &None) {
							file_action = FileActions::PlaySong(song.clone());
						}
					} else {
						if let Some(pl) = playlists.get(element.playlist_position) {
							match render_directory_entry_ui_element(ui, &pl.name, pl.is_open, 0) {
								FileActions::None | FileActions::PlaySong(_) => {},
								/* Playlists cannot be nested so this case is handled the same */
								FileActions::OpenDirectory(_) | FileActions::OpenDirectoryRecursive(_) => {
									file_action = FileActions::OpenPlaylist(element.playlist_position);
								},
								FileActions::CloseDirectory(_) => {
									file_action = FileActions::ClosePlaylist(element.playlist_position);
								},
								FileActions::EnterDirectory(_) => {
									file_action = FileActions::EnterPlaylist(element.playlist_position);
								},
								_ => {},
							}
						} else {
							break; // unreachable(?)
						}
					}
				}
			}
		});
	}
	return file_action;
}

/**
 * Starting to not be a big fan of Rust
 * 
 * Were this in a slightly looser language I'd just do this with a doubly linked list to make every operation O(1) but I'm stuck using v*ctors for this.
 * Doubly linked lists are a pain in the ass in Rust
 * 
 * Maybe I'll go back and do the un-safe stuff should this become a real performance issue, though I imagine there are more pressing areas in this code.
 */
fn range_swap(vec: &mut Vec<String>, start: usize, end_inclusive: usize) {
	if start > end_inclusive {
		for i in (end_inclusive + 1..=start).rev() {
            vec.swap(i, i - 1);
        }
	} else {
		for i in start..end_inclusive {
			vec.swap(i, i + 1);
		}
	}
}

fn drag_vec_element(ui: &mut egui::Ui, playlist_edit_data: &mut PlaylistEditData, row_height: f32, scroll_rect: egui::Rect, scroll_offset: f32) -> usize{
	if let Some(pointer_pos) = ui.input(|i| i.pointer.interact_pos()) && let Some(dragged_element) = playlist_edit_data.current_dragged_element {
		let spacing = ui.spacing().item_spacing.y;
		let pointer_y_in_content = (pointer_pos.y) - scroll_rect.top() + (scroll_offset);

		let height_per_element = row_height + spacing;

		let mut target_row =
					((pointer_y_in_content / height_per_element)).floor() as isize;

		target_row = target_row
				.clamp(0, (playlist_edit_data.edit_vec.len() - 1) as isize);

		let target_row = target_row as usize;

		range_swap(&mut playlist_edit_data.edit_vec, dragged_element, target_row);
		return target_row;
	}
	unreachable!();
}

fn render_playlist_reordering(ui: &mut egui::Ui,
	current_song: &str,
	edit_playlist_data: &mut PlaylistEditData) -> FileActions
{
	let file_action = FileActions::None;
	let row_count = edit_playlist_data.edit_vec.len();
	let row_height = 18.0;
	
	let mut scroll_rect = egui::Rect::NOTHING;
	
	ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);

	let res = egui::ScrollArea::vertical().show_rows(ui, row_height, row_count, |ui, row_range| {
		/* There is an undocumented additional padding added to interactable elements that throws off the math if you don't do this */
		ui.spacing_mut().interact_size.y = row_height;

		/* This is necessary because we need the absolute offset instead of relative */
		ui.set_min_width(ui.available_rect_before_wrap().size().x);

		scroll_rect = ui.clip_rect();

		for row in row_range {
			ui.horizontal(|ui| {
				let is_current_row = if let Some(cur) = edit_playlist_data.current_dragged_element && cur == row {true} else {false};
				let is_dragging = edit_playlist_data.current_dragged_element.is_some();
				let handle = ui.add(egui::Label::new(if is_dragging && is_current_row {"   "} else {":::"}).selectable(!is_dragging).sense(egui::Sense::click()));
				if handle.drag_started() {
					edit_playlist_data.current_dragged_element = Some(row);
				}
				if handle.hovered() {
					ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::Grab);
				}
				if edit_playlist_data.current_dragged_element.is_some() {
					if ui.ctx().input(|i| i.pointer.button_down(egui::PointerButton::Primary))  {
						ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::Grab);
						edit_playlist_data.current_dragged_element = Some(drag_vec_element(ui, edit_playlist_data, row_height, scroll_rect, edit_playlist_data.scroll_offset));
					}
					else {
						edit_playlist_data.current_dragged_element = None;
					}
				}

				if let Some(song) = edit_playlist_data.edit_vec.get(row) {
					if is_current_row {
						ui.add(
					egui::Label::new("")
							.selectable(!is_dragging)
						);
					}
					else {
						if song == current_song {
							ui.add(
						egui::Label::new(egui::RichText::new(extract_file_name(song)).underline().strong())
								.selectable(!is_dragging)
							);
						}
						else {
							ui.add(
						egui::Label::new(extract_file_name(song))
								.selectable(!is_dragging)
							);
						}
					}
				}
			});
		}

		/* Manual placement of drag ghost */
		/* This is done outside the main loop because otherwise it destroys the rendering for some reason */
		if let Some(dragged_row) = edit_playlist_data.current_dragged_element {
			if let Some(pointer_pos) = ui.input(|i| i.pointer.interact_pos()) {
				let rect = egui::Rect::from_min_size(
					egui::pos2(scroll_rect.left(), pointer_pos.y - row_height / 2.0),
					egui::vec2(scroll_rect.width(), row_height),
				);

				let painter = ui.painter();
				painter.rect_filled(rect, 0.0, ui.visuals().selection.bg_fill);
				painter.text(
					rect.left_top(),
					egui::Align2::LEFT_TOP,
					extract_file_name(&edit_playlist_data.edit_vec[dragged_row]),
					egui::FontId::default(),
					ui.visuals().text_color(),
				);
			}
		}
	});
	edit_playlist_data.scroll_offset = res.state.offset.y;
	return file_action;
}

fn add_font(ctx: &egui::Context) {
	let mut fonts = egui::FontDefinitions::default();

	fonts.font_data.insert("fallback_japanese".to_owned(),
		egui::FontData::from_static(include_bytes!("./../fonts/MPLUS1p-Regular.ttf"))
	);
	fonts.font_data.insert("fallback_korean".to_owned(),
		egui::FontData::from_static(include_bytes!("./../fonts/Paperlogy-4Regular.ttf"))
	);

	fonts.families
		.entry(egui::FontFamily::Proportional)
		.or_default()
		.push("fallback_japanese".to_owned());
	fonts.families
		.entry(egui::FontFamily::Proportional)
		.or_default()
		.push("fallback_korean".to_owned());

    ctx.set_fonts(fonts);
}

/**
 * Maybe TODO: add more themes?
 */
fn set_visuals(ctx: &egui::Context, theme: &ThemePref) {
	if *theme == ThemePref::DARK {
		let mut vis = egui::Visuals::dark();
		vis.hyperlink_color = egui::Color32::from_rgb(180, 180, 255);
		ctx.set_visuals(vis);
	} else {
		let mut vis = egui::Visuals::light();
		// vis.widgets.active.weak_bg_fill = Color32::from_rgb(150, 150, 150);
		vis.widgets.inactive.weak_bg_fill = egui::Color32::from_rgb(200, 200, 200);
		vis.hyperlink_color = egui::Color32::from_rgb(100, 100, 255);
		ctx.set_visuals(vis);
	}
}

fn default_on_finish_to_str(behavior: &LoopBehavior) -> &'static str {
	match behavior {
		LoopBehavior::Stop => {
			"Stop"
		},
		LoopBehavior::Loop => {
			"Loop"
		},
		LoopBehavior::Next => {
			"Next"
		},
		LoopBehavior::Shuffle => {
			"Shuffle"
		},
	}
}

fn prev_behavior_to_str(p: &PrevBehavior) -> &str {
	return match *p {
		PrevBehavior::Above => {
			"Above"
		},
		PrevBehavior::History => {
			"History"
		},
	};
}

fn write_internal_data(path: &str, persistent_data: &PersistentData) -> Result<(), Box<dyn std::error::Error>>{
	use std::io::prelude::*;
	let file = std::fs::File::create(path);
	if let Ok(mut file) = file {
		let mut data_to_write = String::new();
		data_to_write = format!("VERSION: {}\n", CURRENT_VERSION);

		data_to_write = format!("{}{}", data_to_write, "SETTINGS\n");
		data_to_write = format!("{}Theme: {}\n", data_to_write, theme_to_str(&persistent_data.theme));
		data_to_write = format!("{}Default Directory: {}\n", data_to_write, persistent_data.default_directory);
		data_to_write = format!("{}Default End Behavior: {}\n", data_to_write, default_on_finish_to_str(&persistent_data.default_on_finish));
		data_to_write = format!("{}Default Prev Behavior: {}\n", data_to_write, prev_behavior_to_str(&persistent_data.prev_behavior));
		data_to_write = format!("{}Default Volume: {}\n", data_to_write, persistent_data.default_volume.to_string());
		data_to_write = format!("{}Hide Directories: {}\n", data_to_write, 
			if persistent_data.hide_directories_by_default {"true"}
			else {"false"}
		);
		data_to_write = format!("{}Shuffle Memory: {}\n", data_to_write, persistent_data.shuffle_memory);

		data_to_write = format!("{}{}", data_to_write, "PLAYLISTS\n");

		for playlist in &persistent_data.playlists {
			data_to_write = format!("{}Playlist: {}\n", data_to_write, playlist.name);
			
			for song in &playlist.songs {
				data_to_write = format!("{}{}\n", data_to_write, song);
			}
		}
		file.write_all(data_to_write.as_bytes())?;
	}
	return Ok(());
}

/**
* Returns None on success. Otherwise, returns a string with the relevant error.
*/
fn run_install(install_data: &InstallerData, persistent_data: &PersistentData) -> Option<String> {
	let fp = build_full_filepath(&install_data.install_path, "Pinetree");

	let install_location_result = std::fs::create_dir(fp);
	if let Ok(_) = install_location_result {
		let fp_exe = build_full_filepath("Pinetree", "pinetree.exe");
		let target_exe_path = build_full_filepath(&install_data.install_path, &fp_exe);

		if let Ok(current_exe) = std::env::current_exe() {
			if let Ok(_) = std::fs::copy(&current_exe, &target_exe_path) {
				// Nothing: Success
				// Continue as normal
			} else {
				return Some(format!("Failed to write the executable to the install directory (genuinely how did you even trigger this)"));
			}
		} else {
			return Some(format!("Failed to retrieve the current executable (genuinely how did you even trigger this)"));
		}

		let fp_data = build_full_filepath("Pinetree", "internal_pinetree_data.txt");
		let target_data_path = build_full_filepath(&install_data.install_path, &fp_data);

		if let Ok(_) = write_internal_data(&target_data_path, &persistent_data) {
			return None; // Success
		} else {
			return Some(format!("Failed to write persistent data file (unknown error)"));
		}
	} else {
		return Some(format!("Failed to write to directory (insufficient permissions?)"));
	}
}

#[derive(Clone)]
enum PlaylistElementType {
	Song,
	_Directory,
}

#[derive(PartialEq)]
#[derive(Clone)]
enum PlaylistEditMode {
	AddRemove,
	Reorder,
	Remove,
}

/**
 * Playlist behavior:
 * Step 1: Add all songs in playlist to a hashmap
 * - Checking/unchecking adds or removes it from the hashmap
 * Step 2: Saving copies over first from the original (checking if it still exists)
 * Step 3: Write out to the internal data file incrementally
 */
struct PlaylistEditData {
	playlist_index: usize,
	// Gets set after the fact (for editing)
	playlist_name: String,
	edit_map: HashMap<String, PlaylistElementType>,
	edit_vec: Vec<String>,
	last_touched_index: usize,
	mode: PlaylistEditMode,
	current_dragged_element: Option<usize>,
	scroll_offset: f32,

	removal_map: HashMap<String, PlaylistElementType>,
}

fn playlist_edit_data_to_str(mode: &PlaylistEditMode) -> &'static str {
	return match mode {
		PlaylistEditMode::AddRemove => {"Select songs"},
		PlaylistEditMode::Reorder => {"Reorder songs"},
		PlaylistEditMode::Remove => {"Remove songs"},
	}
}

/**
 * TODO: Add directory support
 */
fn init_playlist_edit_data(playlists: &Vec<Playlist>, index: usize) -> PlaylistEditData {
	let mut new_edit_data = PlaylistEditData {
		playlist_index: index,
		playlist_name: "".to_string(),
		edit_map: HashMap::<String, PlaylistElementType>::new(),
		edit_vec: Vec::<String>::new(),
		last_touched_index: 0,
		mode: PlaylistEditMode::AddRemove,
		current_dragged_element: None,
		scroll_offset: 0.0,
		removal_map: HashMap::<String, PlaylistElementType>::new(),
	};
	if let Some(playlist) = playlists.get(index) {
		new_edit_data.playlist_name = playlist.name.to_string();
		for song in &playlist.songs {
			new_edit_data.edit_map.insert(song.clone(), PlaylistElementType::Song);
			new_edit_data.edit_vec.push(song.clone());
		}
	}
	return new_edit_data;
}

/**
 * Builds an ordered vec that looks like the following:
 * - Existing elements preserve their place in the new vec
 * - Added elements are on the end, sorted alphabetically
 */
fn rebuild_ordered_vec(existing_ordered_vec: &mut Vec<String>, existence_map: &mut HashMap<String, PlaylistElementType>) -> Vec<String> {
	let mut new_ordered_vec = Vec::<String>::new();
	let mut existence_map_new = HashMap::<String, PlaylistElementType>::new();
	for song in existing_ordered_vec {
		if existence_map.contains_key(song) {
			new_ordered_vec.push(song.clone());
			existence_map_new.insert(song.clone(), existence_map.get(song).unwrap_or(&PlaylistElementType::Song).clone());
		}
	}

	let mut keymap = Vec::<String>::new();
	for (song, _) in existence_map {
		if !existence_map_new.contains_key(song) {
			keymap.push(song.clone());
		}
	}

	// TODO: Fix this to remove all the copying
	keymap.sort_by_key(|name| name.to_lowercase());

	/* Combine */
	for element in keymap {
		new_ordered_vec.push(element);
	}

	return new_ordered_vec;
}

enum SaveError {
	None,
	Success,
	Error(String),
}

#[cfg(target_os = "windows")]
fn set_pin_mode(hwnd: windows_sys::Win32::Foundation::HWND, pin: bool) {
	use windows_sys::Win32::UI::WindowsAndMessaging::*;

    unsafe {
        SetWindowPos(
            hwnd,
            if pin { HWND_TOPMOST } else { HWND_NOTOPMOST },
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE,
        );
    }
}

fn is_user_typing(ui: &mut egui::Ui) -> bool {
	let mut res = false;
	ui.input(|i| {
		for event in &i.events {
			if let egui::Event::Text(text) = event && !res{
				res = text.len() > 0;
			}
		}
	});
	return res;
} 

fn search_bar(ui: &mut egui::Ui, ctx: &egui::Context, search_text: &mut String) -> egui::Response {
	let response = ui.add(egui::TextEdit::singleline(search_text)
		.hint_text("Search..."))
		.on_hover_text("Searches based on the file name");

	let mut focused = false;
	ctx.memory( |memory| {
		focused = memory.focused().is_some();
	});
	
	if is_user_typing(ui) && !focused {
		/* Getting this shit to work was S tier ragebait */
		/* By default requesting focus in egui sets the cursor to the *beginning* of the text edit - not the end. */
		ui.input(|i| {
			for event in &i.events {
				if let egui::Event::Text(text) = event {
					*search_text = format!("{}{}", search_text, text);
				}
			}
		});
		let mut state = egui::TextEdit::load_state(ui.ctx(), response.id).unwrap_or_default();
		let end_pos = search_text.chars().count();
		state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(end_pos))));
		state.store(ui.ctx(), response.id);
	}

	let to_req = (is_user_typing(ui) && !focused) || ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::F));

	if to_req {
		response.request_focus();
	}
	return response;
}

impl eframe::App for MyApp {
	fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
		if !self.first_frame_rendered {
			set_visuals(&ctx, &self.persistent_data.theme);
			/* Solved for now? */
			ctx.set_pixels_per_point(1.25);
			self.first_frame_rendered = true;
			add_font(ctx);
			send_audio_signal(&self.audio_message_channel, MessageToAudio::UpdateEndBehavior(clone_loop_behavior(&self.loop_behavior)));
			send_audio_signal(&self.audio_message_channel, MessageToAudio::UpdateVolume(volume_curve(self.song_volume)));
			send_audio_signal(&self.audio_message_channel, MessageToAudio::UpdatePrevBehavior(self.prev_behavior));
		}
		// 8 fps
		let audio_data = request_rodio_data(&mut self.audio_message_channel, &mut self.audio_receive_channel);
		ctx.request_repaint_after(std::time::Duration::from_millis(125));
		let height = ctx.available_rect().height();
		if height > 80.0 {
			egui::TopBottomPanel::top("Header").show(ctx, |ui| {
				ui.add_space(5.0);
				ui.horizontal(|ui| {
					ui.heading("Pinetree Player");
					ui.with_layout(egui::Layout::right_to_left(egui::Align::RIGHT), |ui| {
						if ui.button("←").on_hover_text_at_pointer("Backs up to the folder above").clicked() {
							self.current_song_folder = song_folder_go_up(&self.current_song_folder);
							self.directory_tree = None;
							self.searched_directory_tree = None;
							self.active_directory_filepath = self.current_song_folder.clone();
							self.active_search_text = "".to_string();
						}
						if !self.hide_fp {
							let song_folder_field = ui.add(egui::TextEdit::singleline(&mut self.current_song_folder)
							.hint_text("Song folder..."))
							.on_hover_text("The file path of the current open folder.\nRelative to the file path of the executable");
		
							if song_folder_field.lost_focus() && song_folder_field.ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
								self.directory_tree = None;
								self.searched_directory_tree = None;
								self.active_directory_filepath = self.current_song_folder.clone();
								self.active_search_text = "".to_string();
							}
						} else {
							ui.horizontal(|ui| {
								ui.disable();
								let mut hidden_str = "(Hidden)".to_string();
								let song_folder_field = ui.add(egui::TextEdit::singleline(&mut hidden_str));
			
								if song_folder_field.lost_focus() && song_folder_field.ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
									self.directory_tree = None;
									self.searched_directory_tree = None;
									self.active_directory_filepath = self.current_song_folder.clone();
									self.active_search_text = "".to_string();
								}
								song_folder_field
							});
						}
						if !self.hide_fp {
							let text = egui::RichText::new("Hide").line_height(Some(15.0));
							if ui.button(text).clicked() {
								self.hide_fp = true;
							}
						} else {
							let text = egui::RichText::new("Show").line_height(Some(15.0));
							if ui.button(text).clicked() {
								self.hide_fp = false;
							}
						}
						
					});
				});
				ui.add_space(5.0);
			});
		}
		
		let active_directory: Option<&Directory> = {
			if let Some(dir) = self.directory_map.get(&self.active_directory_filepath) {
				Some(dir)
			} else {
				init_directory_at_filepath(&self.active_directory_filepath, &mut self.directory_map);
				self.directory_map.get(&self.active_directory_filepath)
			}
		};

		egui::TopBottomPanel::bottom("Player").show(ctx, |ui| {
			ui.horizontal(|ui| {
				ui.label("On finish: ");
				let ob = clone_loop_behavior(&self.loop_behavior);
				egui::ComboBox::from_label("")
					.selected_text(loop_behavior_to_str(&self.loop_behavior))
					.show_ui(ui, |ui| {
						ui.selectable_value(&mut self.loop_behavior, LoopBehavior::Stop, "Stop");
						ui.selectable_value(&mut self.loop_behavior, LoopBehavior::Loop, "Loop");
						ui.selectable_value(&mut self.loop_behavior, LoopBehavior::Shuffle, "Shuffle");
						ui.selectable_value(&mut self.loop_behavior, LoopBehavior::Next, "Next");
					}
				);
				if ob != self.loop_behavior {
					send_audio_signal(&self.audio_message_channel, MessageToAudio::UpdateEndBehavior(clone_loop_behavior(&self.loop_behavior)));
				}
				ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
				if let Some(err) = audio_data.error_message {
					if ui.button(egui::RichText::new("x").color(egui::Color32::RED).line_height(Some(16.0))).clicked() {
						send_audio_signal(&self.audio_message_channel, MessageToAudio::ClearError);
					}
					ui.label(egui::RichText::new(err).color(egui::Color32::RED));
				}
				else {
					ui.label(if audio_data.song_name == "" {format!("No song playing")} else {format!("Now playing: {}", extract_file_name(&audio_data.song_name))});
				}
			});
			ui.horizontal(|ui| {
				let icon_size = 16.0;
				let line_height = Some(icon_size + 3.0);
				let pause_play_text = {
					let raw_text = if audio_data.is_paused {format!("▶")} else {format!("⏸")};
					egui::RichText::new(raw_text).line_height(line_height).size(icon_size)
				};
				let prev_text = {
					let raw_text = "⏮";
					egui::RichText::new(raw_text).line_height(line_height).size(icon_size)
				};
				let skip_text = {
					let raw_text = "⏭";
					egui::RichText::new(raw_text).line_height(line_height).size(icon_size)
				};

				if ui.button(prev_text).clicked() || ctx.input_mut(|i| i.consume_shortcut(&PREV_SONG)) {
					send_audio_signal(&self.audio_message_channel, MessageToAudio::PreviousSong);
				}
				if ui.button(pause_play_text).clicked() || ctx.input_mut(|i| i.consume_shortcut(&PAUSE)) {
					send_audio_signal(&self.audio_message_channel, MessageToAudio::TogglePause);
				}
				if ui.button(skip_text).clicked() || ctx.input_mut(|i| i.consume_shortcut(&NEXT_SONG)) {
					send_audio_signal(&self.audio_message_channel, MessageToAudio::SongEnd);
				}

				let prev_vol = self.song_volume;
				let volume_text = {
					let raw_text = if self.song_volume > 0.5 {"🔊"}
						else if self.song_volume > 0.25 {"🔉"}
						else if self.song_volume > -0.195 {"🔈"}
						else {"🔇"};
					egui::RichText::new(raw_text).line_height(line_height).size(icon_size)
				};
				ui.label(volume_text);
				ui.add_sized([120.0, ui.spacing().interact_size.y],
					egui::Slider::new(&mut self.song_volume, -0.2..=1.0)
					.show_value(false)
					.trailing_fill(true)
				);
				if prev_vol != self.song_volume {
					send_audio_signal(&self.audio_message_channel, MessageToAudio::UpdateVolume(volume_curve(self.song_volume)));
				}

				let mut playback_pos = audio_data.playback_position;
				let secs = playback_pos / 1000;

				/* When the seconds < 10, it is displayed as e.g. 3:05 and not 3:5 */
				let timestamp = {
					let minute_count = secs / 60;
					let remaining_seconds = secs % 60;
					format!("{}:{}{}", minute_count, remaining_seconds / 10, remaining_seconds % 10)
				};
				ui.label(timestamp);

				let remaining_width = ui.available_width();
				ui.spacing_mut().slider_width = remaining_width;

				let seeker = ui.add_sized([remaining_width, ui.spacing().interact_size.y],
					egui::Slider::new(&mut playback_pos, 0..=(audio_data.song_length))
					.handle_shape(egui::style::HandleShape::Rect { aspect_ratio: 1.0 })
					.show_value(false)
					.trailing_fill(true)
					// Fill color can be adjusted with ui.visuals_mut().selection.bg_fill = Color32::{INSERT COLOR HERE};
				);

				if seeker.dragged() {
					send_audio_signal(&self.audio_message_channel, MessageToAudio::Seek(playback_pos));
				}
				if seeker.drag_stopped() {
					send_audio_signal(&self.audio_message_channel, MessageToAudio::SeekStop);
				}

				let mut focused = false;
				ctx.memory( |memory| {
					focused = !memory.focused().is_none();
				});
				if !focused {
					if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
						let seek_pos = audio_data.playback_position + 5000; // 5 seconds
						send_audio_signal(&self.audio_message_channel, MessageToAudio::Seek(seek_pos))
					}
					if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
						let seek_pos = audio_data.playback_position.saturating_sub(5000); // 5 seconds
						send_audio_signal(&self.audio_message_channel, MessageToAudio::Seek(seek_pos))
					}
				}
			});
		});


		let mut request_refresh = false;

		let mut file_action = FileActions::None;

		egui::SidePanel::left("left_panel").default_width(250.0).width_range(250.0..=650.0).show(ctx, |ui| {
			// LOL this has to be centered or else egui cannot resize the panel. This is so dumb lmao
			ui.vertical_centered(|ui| {
				ui.heading("Songs");
			});
			match self.browse_mode {
				LeftPanelMode::Files => {
					ui.horizontal(|ui| {
						if let None = &self.edit_playlist_data {
							egui::ComboBox::from_label("")
								.selected_text(song_browse_mode_to_str(&self.browse_mode))
								.show_ui(ui, |ui| {
									ui.selectable_value(&mut self.browse_mode, LeftPanelMode::Files, "Files");
									ui.selectable_value(&mut self.browse_mode, LeftPanelMode::Playlists, "Playlists");
								});
						}
						/* TODO */
						// if ui.button("Advanced").clicked() {
						// 	self.advanced_search_active = !self.advanced_search_active;
						// }
						request_refresh = ui.button("Refresh").clicked() || ctx.input_mut(|i| i.consume_shortcut(&REFRESH));
					});

					if self.browse_mode == LeftPanelMode::Playlists {
						if let Some(tree) = &self.playlist_tree {
							let mut collection = Vec::<String>::new();
							let mut new_current_location = None;
							let mut i = 0;
							for el in tree {
								if let Some(name) = &el.song_name {
									if new_current_location == None && *name == audio_data.song_name {
										new_current_location = Some(i);
									}
									collection.push(name.to_string());
									i += 1;
								}
							}
							send_audio_signal(&self.audio_message_channel, MessageToAudio::SetSongCollection(collection, new_current_location));
						}
					}

					ui.horizontal(|ui| {
						let response = search_bar(ui, ctx, &mut self.search_text);

						if response.lost_focus() && response.ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
							if let Some(rawtree) = &self.directory_tree {
								if let Some(tree) = &self.searched_directory_tree {
									if let Some(index) = tree.get(0)
									&& let Some(thing) = rawtree.get(*index) {
										if thing.is_dir {
											file_action = FileActions::EnterDirectory(thing.name.clone());
										} else {
											file_action = FileActions::PlaySong(thing.name.clone());
										}
									}
								} else {
									if let Some(thing) = rawtree.get(0) {
										if thing.is_dir {
											file_action = FileActions::EnterDirectory(thing.name.clone());
										} else {
											file_action = FileActions::PlaySong(thing.name.clone());
										}
									}
								}
								response.request_focus();
							}
						}
		
						if self.search_text == "" {
							self.searched_directory_tree = None;
							self.searched_playlist_tree = None;
						}
					});
					if self.advanced_search_active {
						ui.add(egui::TextEdit::singleline(&mut self.genre_search_text)
							.hint_text("Genre..."))
							.on_hover_text("Searches based on the genre name");
						ui.add(egui::TextEdit::singleline(&mut self.artist_search_text)
							.hint_text("Artist..."))
							.on_hover_text("Searches based on the artist name");
					}
					ui.horizontal(|ui| {
						ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
						if ui.button("↑").clicked() {
							self.current_song_folder = song_folder_go_up(&self.current_song_folder);
							self.directory_tree = None;
							self.searched_directory_tree = None;
							self.active_directory_filepath = self.current_song_folder.clone();
							self.active_search_text = "".to_string();
							self.search_text = "".to_string();
							request_refresh = true;
						}
						ui.label(egui::RichText::new(extract_folder_name(&self.active_directory_filepath)).strong());
					});
					ui.add_space(5.0);

					if let Some(active_directory) = active_directory {
						/* Directory tree initialization in case it is null */
						if let None = self.directory_tree {
							let mut new_tree = Vec::<DirTreeElement>::new();
							let mut collection = Vec::<String>::new();
							get_dir_tree_elements(&mut new_tree, &active_directory.filepath_identifier, &self.directory_map, 0);
							let mut new_current_location: Option<usize> = None;

							for el in &new_tree {
								if !el.is_dir {
									if let None = new_current_location && el.name == audio_data.song_name {
										new_current_location = Some(collection.len());
									}
									collection.push(el.name.clone());
								}
							}
							send_audio_signal(&self.audio_message_channel, MessageToAudio::SetSongCollection(collection, new_current_location));
							self.directory_tree = Some(new_tree);
						}
						let searching = self.search_text != "";
						
						if let Some(directory_tree_elements) = &self.directory_tree {
							if searching && self.active_search_text != self.search_text {
								self.active_search_text = self.search_text.clone();
								let compare_to = self.active_search_text.to_lowercase();
								self.searched_directory_tree = {
									let mut vec = Vec::<usize>::new();
									for i in 0..directory_tree_elements.len() {
										if let Some(element) = directory_tree_elements.get(i)
										&& extract_file_name(&element.name.to_lowercase()).contains(&compare_to) {
											vec.push(i);
										}
									}
									Some(vec)
								};
							}
						}
						let faction = render_directory_elements(ui, &self.directory_tree, &self.searched_directory_tree, &audio_data.song_name, &self.edit_playlist_data);
						if file_action == FileActions::None {
							file_action = faction;
						}
					} else {
						ui.label("Error: Directory does not exist");
					}
				},
				LeftPanelMode::Playlists => {
					ui.horizontal(|ui| {
						if let None = &self.edit_playlist_data {
							egui::ComboBox::from_label("")
								.selected_text(song_browse_mode_to_str(&self.browse_mode))
								.show_ui(ui, |ui| {
									ui.selectable_value(&mut self.browse_mode, LeftPanelMode::Files, "Files");
									ui.selectable_value(&mut self.browse_mode, LeftPanelMode::Playlists, "Playlists");
								});
						}
						// if ui.button("Advanced").clicked() {
						// 	self.advanced_search_active = !self.advanced_search_active;
						// }
						request_refresh = ui.button("Refresh").clicked() || ctx.input_mut(|i| i.consume_shortcut(&REFRESH));
					});
					if self.browse_mode == LeftPanelMode::Files {
						if let Some(dirtree) = &self.directory_tree {
							let mut collection = Vec::<String>::new();
							let mut new_current_location: Option<usize> = None;

							for el in dirtree {
								if !el.is_dir {
									if let None = new_current_location && el.name == audio_data.song_name {
										new_current_location = Some(collection.len());
									}
									collection.push(el.name.clone());
								}
							}
							send_audio_signal(&self.audio_message_channel, MessageToAudio::SetSongCollection(collection, new_current_location));
						}
					}

					ui.horizontal(|ui| {
						let response = search_bar(ui, ctx, &mut self.search_text);

						if response.lost_focus() && response.ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
							if let Some(rawtree) = &self.playlist_tree {
								if let Some(tree) = &self.searched_playlist_tree {
									if let Some(index) = tree.get(0)
									&& let Some(thing) = rawtree.get(*index) {
										if let Some(song_name) = &thing.song_name {
											file_action = FileActions::PlaySong(song_name.clone());
										} else {
											file_action = FileActions::EnterPlaylist(thing.playlist_position);
										}
									}
								} else {
									if let Some(thing) = rawtree.get(0) {
										if let Some(song_name) = &thing.song_name {
											file_action = FileActions::PlaySong(song_name.clone());
										} else {
											file_action = FileActions::EnterPlaylist(thing.playlist_position);
										}
									}
								}
								response.request_focus();
							}
						}
		
						if self.search_text == "" {
							self.searched_directory_tree = None;
							self.searched_playlist_tree = None;
						}
					});
					
					if let None = self.active_playlist_index {
						if ui.button("New").clicked() {
							self.persistent_data.playlists.push(Playlist {
								is_open: false,
								name: "New playlist".to_string(),
								songs: Vec::<String>::new(),
							});
		
							self.browse_mode = LeftPanelMode::SelectSongs;
		
							self.edit_playlist_data = Some(init_playlist_edit_data(&self.persistent_data.playlists, self.persistent_data.playlists.len() - 1));
						}
					}
					if let None = self.playlist_tree {
						if let Some(active_playlist_index) = self.active_playlist_index {
							let mut tree = Vec::<PlaylistTreeElement>::new();
							let mut collection = Vec::<String>::new();
							let mut new_current_location: Option<usize> = None;
							let playlists = &mut self.persistent_data.playlists;
							playlists.sort_by_key(|playlist| playlist.name.to_lowercase());
							if let Some(playlist) = playlists.get(active_playlist_index) {
								for song in &playlist.songs {
									tree.push(PlaylistTreeElement {
										song_name: Some(song.clone()),
										playlist_position: 0,
									});
									if let None = new_current_location && *song == audio_data.song_name {
										new_current_location = Some(collection.len());
									}
									collection.push(song.to_string());
								}
								self.playlist_tree = Some(tree);
								send_audio_signal(&self.audio_message_channel, MessageToAudio::SetSongCollection(collection, new_current_location));
							} else {
								ui.label(format!("Unknown playlist error"));
								self.playlist_tree = None;
							}
						} else {
							let tree = build_playlist_tree(&self.persistent_data.playlists);
							let mut new_collection = Vec::<String>::new();
							let mut new_current_location = None;
							let mut i = 0;
							
							for thing in &tree {
								if let Some(name) = &thing.song_name {
									new_collection.push(name.clone());
									if *name == audio_data.song_name {
										new_current_location = Some(i);
									}
									i += 1;
								}
							}
							self.playlist_tree = Some(tree);
							send_audio_signal(&self.audio_message_channel, MessageToAudio::SetSongCollection(new_collection, new_current_location));
						}
					}

					if let Some(active_playlist_index) = self.active_playlist_index {
						if let Some(playlist) = self.persistent_data.playlists.get(active_playlist_index) {
							ui.add_space(1.0);
							ui.label(egui::RichText::new(format!("{}", playlist.name)).strong());
							ui.add_space(1.0);
						} else {
							ui.label(format!("Unknown playlist error"));
						}
						ui.add_space(5.0);
						ui.horizontal(|ui| {
							if ui.button("Go back").clicked() {
								self.search_text = "".to_string();
								request_refresh = true;
							}
							if ui.button("Edit songs").clicked() {
								request_refresh = true;
								self.browse_mode = LeftPanelMode::SelectSongs;
								self.edit_playlist_data = Some(init_playlist_edit_data(&self.persistent_data.playlists, active_playlist_index));
							}
							if ui.button("Delete").clicked() {
								self.browse_mode = LeftPanelMode::DeletePlaylist;
							}
						});
						ui.add_space(5.0);
					}

					if let Some(playlist_tree) = &self.playlist_tree && playlist_tree.len() > 0 {
						let searching = self.search_text != "";
						if searching {
							if self.search_text != self.active_search_text_playlists {
								self.active_search_text_playlists = self.search_text.clone();
								self.searched_playlist_tree = None;
							}
							let compare_to = self.active_search_text_playlists.to_lowercase();
							if let None = self.searched_playlist_tree {
								let mut tmp_vec = Vec::<usize>::new();
								let mut position = 0;
								for element in playlist_tree {
									if let Some(name) = &element.song_name && extract_file_name(&name.to_lowercase()).contains(&compare_to) {
										tmp_vec.push(position);
									} else if let Some(playlist) = self.persistent_data.playlists.get(element.playlist_position)
									&& playlist.name.to_lowercase().contains(&compare_to) {
										tmp_vec.push(position);
									}
									position += 1;
								}
								self.searched_playlist_tree = Some(tmp_vec);
							}
						}
						if let Some(edit_playlist_data) = &mut self.edit_playlist_data {
							let faction = render_playlist_reordering(ui, &audio_data.song_name, edit_playlist_data);
							if file_action == FileActions::None {
								file_action = faction;
							}
						} else {
							let song_depth = if let Some(_) = self.active_playlist_index {0} else {1};
							let faction = render_playlist_elements(ui, &self.playlist_tree, &self.searched_playlist_tree, &self.persistent_data.playlists, song_depth, &audio_data.song_name);
							if file_action == FileActions::None {
								file_action = faction;
							}
						}
					} else {
						ui.label("No saved playlists found");
					}
				},
				LeftPanelMode::DeletePlaylist => {
					if let Some(playlist_index) = self.active_playlist_index {
						ui.vertical_centered(|ui| {
							if let Some(playlist) = self.persistent_data.playlists.get(playlist_index) {
								ui.label(format!("Are you sure you want to delete the playlist {}?", playlist.name));
							}
							if ui.button("Yes").clicked() {
								self.persistent_data.playlists.remove(playlist_index);
								self.active_playlist_index = None;
								request_refresh = true;
								self.browse_mode = LeftPanelMode::Playlists;
								
								let write_to = build_full_filepath(&self.installed_location, "internal_pinetree_data.txt");
								if let Ok(_) = write_internal_data(&write_to, &self.persistent_data) {
									
								} else {
									println!("Error in saving");
								}
							}
							if ui.button("No").clicked() {
								self.browse_mode = LeftPanelMode::Playlists;
							}
						});
					}
					else {
						self.browse_mode = LeftPanelMode::Playlists;
					}
				},
				LeftPanelMode::SelectSongs => {
					ui.horizontal(|ui| {
						// if ui.button("Advanced").clicked() {
						// 	self.advanced_search_active = !self.advanced_search_active;
						// }
						request_refresh = ui.button("Refresh").clicked() || ctx.input_mut(|i| i.consume_shortcut(&REFRESH));
					});

					ui.horizontal(|ui| {
						let _ = search_bar(ui, ctx, &mut self.search_text);
		
						if self.search_text == "" {
							self.searched_directory_tree = None;
							self.searched_playlist_tree = None;
						}
					});
					if self.advanced_search_active {
						ui.add(egui::TextEdit::singleline(&mut self.genre_search_text)
							.hint_text("Genre..."))
							.on_hover_text("Searches based on the genre name");
						ui.add(egui::TextEdit::singleline(&mut self.artist_search_text)
							.hint_text("Artist..."))
							.on_hover_text("Searches based on the artist name");
					}
					ui.add_space(2.5);
					if let Some(playlist_edit_data) = &mut self.edit_playlist_data {

						ui.add(egui::TextEdit::singleline(&mut playlist_edit_data.playlist_name).hint_text("Playlist name..."));
						let mut remove: bool = false;
						ui.horizontal(|ui| {
							if ui.button("Cancel").clicked() {
								remove = true;
							}
							/*
							* When in add mode: Only push to hashmap
							* When in reorder mode: First rebuild the vec from the hash map, then only do reordering.
							*/
							ui.add_space(2.5);
							if ui.button("Save").clicked() {
								if let Some(pl) = self.persistent_data.playlists.get_mut(playlist_edit_data.playlist_index) {
									pl.is_open = true;
									pl.name = playlist_edit_data.playlist_name.clone();

									pl.songs = rebuild_ordered_vec(&mut playlist_edit_data.edit_vec, &mut playlist_edit_data.edit_map);
								}
								remove = true;
							}
							let prev =  playlist_edit_data.mode.clone();
							egui::ComboBox::from_label(" ")/* Lmao */
								.selected_text(
									playlist_edit_data_to_str(&playlist_edit_data.mode)
								)
								.show_ui(ui, |ui| {
									ui.selectable_value(&mut playlist_edit_data.mode, PlaylistEditMode::AddRemove, playlist_edit_data_to_str(&PlaylistEditMode::AddRemove));
									ui.selectable_value(&mut playlist_edit_data.mode, PlaylistEditMode::Reorder, playlist_edit_data_to_str(&PlaylistEditMode::Reorder));
									ui.selectable_value(&mut playlist_edit_data.mode, PlaylistEditMode::Remove, playlist_edit_data_to_str(&PlaylistEditMode::Remove));
							});
							match playlist_edit_data.mode {
								PlaylistEditMode::AddRemove => {
									self.browse_mode = LeftPanelMode::SelectSongs;
								},
								PlaylistEditMode::Reorder => {
									self.browse_mode = LeftPanelMode::ReorderSongs;
								},
								PlaylistEditMode::Remove => {
									self.browse_mode = LeftPanelMode::RemoveSongs;
								},
							}
							if prev != playlist_edit_data.mode {
								match playlist_edit_data.mode {
									/* Shouldn't need to do anything here */
									PlaylistEditMode::AddRemove => {},
									PlaylistEditMode::Reorder => {
										playlist_edit_data.edit_vec = rebuild_ordered_vec(&mut playlist_edit_data.edit_vec, &mut playlist_edit_data.edit_map);
									},
									PlaylistEditMode::Remove => {
										playlist_edit_data.edit_vec = rebuild_ordered_vec(&mut playlist_edit_data.edit_vec, &mut playlist_edit_data.edit_map);
										// playlist_edit_data.removal_map = HashMap::<String, PlaylistElementType>::new();
									},
								}
							}
						});
						ui.add_space(5.0);
						if remove {
							if let Some(pl) = self.persistent_data.playlists.get(playlist_edit_data.playlist_index) && pl.songs.len() == 0 {
								self.persistent_data.playlists.remove(playlist_edit_data.playlist_index);
							} else {
								self.active_playlist_index = Some(playlist_edit_data.playlist_index);
							}
							self.browse_mode = LeftPanelMode::Playlists;
							self.playlist_tree = None; // Force reload
							self.edit_playlist_data = None;

							if self.persistent_data.data_file_exists {
								let write_to = build_full_filepath(&self.installed_location, "internal_pinetree_data.txt");
								if let Ok(_) = write_internal_data(&write_to, &self.persistent_data) {
									/* TODO */
								} else {
									println!("Error in saving");
								}
							}
						}

						if let Some(active_directory) = active_directory {
							/* Directory tree initialization in case it is null */
							if let None = self.directory_tree {
								let mut new_tree = Vec::<DirTreeElement>::new();
								let mut collection = Vec::<String>::new();
								get_dir_tree_elements(&mut new_tree, &active_directory.filepath_identifier, &self.directory_map, 0);
								let mut new_current_location: Option<usize> = None;

								for el in &new_tree {
									if !el.is_dir {
										if let None = new_current_location && el.name == audio_data.song_name {
											new_current_location = Some(collection.len());
										}
										collection.push(el.name.clone());
									}
								}
								send_audio_signal(&self.audio_message_channel, MessageToAudio::SetSongCollection(collection, new_current_location));
								self.directory_tree = Some(new_tree);
							}
							let searching = self.search_text != "";
							
							if let Some(directory_tree_elements) = &self.directory_tree {
								if searching && self.active_search_text != self.search_text {
									self.active_search_text = self.search_text.clone();
									let compare_to = self.active_search_text.to_lowercase();
									self.searched_directory_tree = {
										let mut vec = Vec::<usize>::new();
										for i in 0..directory_tree_elements.len() {
											if let Some(element) = directory_tree_elements.get(i)
											&& extract_file_name(&element.name.to_lowercase()).contains(&compare_to) {
												vec.push(i);
											}
										}
										Some(vec)
									};
								}
							}
							file_action = render_directory_elements(ui, &self.directory_tree, &self.searched_directory_tree, &audio_data.song_name, &self.edit_playlist_data);
						} else {
							ui.label("Error: Directory does not exist");
						}
					}
					else {
						/* Error case (unreachable?) */
						self.browse_mode = LeftPanelMode::Files;
					}
				},
				LeftPanelMode::RemoveSongs => {
					ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
					if let Some(playlist_edit_data) = &mut self.edit_playlist_data {
						ui.add(egui::TextEdit::singleline(&mut playlist_edit_data.playlist_name).hint_text("Playlist name..."));
						let mut remove: bool = false;
						ui.horizontal(|ui| {
							if ui.button("Cancel").clicked() {
								remove = true;
							}
							/*
							* When in add mode: Only push to hashmap
							* When in reorder mode: First rebuild the vec from the hash map, then only do reordering.
							*/
							ui.add_space(2.5);
							if ui.button("Save").clicked() {
								if let Some(pl) = self.persistent_data.playlists.get_mut(playlist_edit_data.playlist_index) {
									pl.is_open = true;
									pl.name = playlist_edit_data.playlist_name.clone();

									let mut new_vec: Vec<String> = Vec::<String>::new();
									for song in &playlist_edit_data.edit_vec {
										if !playlist_edit_data.removal_map.contains_key(song) {
											playlist_edit_data.removal_map.remove(song);
											new_vec.push(song.clone());
										} else {
											playlist_edit_data.edit_map.remove(song);
										}
									}
									playlist_edit_data.edit_vec = new_vec;

									pl.songs = rebuild_ordered_vec(&mut playlist_edit_data.edit_vec, &mut playlist_edit_data.edit_map);

									playlist_edit_data.removal_map = HashMap::<String, PlaylistElementType>::new();
								}
								remove = true;
							}
							let prev =  playlist_edit_data.mode.clone();
							egui::ComboBox::from_label(" ")/* Lmao */
								.selected_text(
									playlist_edit_data_to_str(&playlist_edit_data.mode)
								)
								.show_ui(ui, |ui| {
									ui.selectable_value(&mut playlist_edit_data.mode, PlaylistEditMode::AddRemove, playlist_edit_data_to_str(&PlaylistEditMode::AddRemove));
									ui.selectable_value(&mut playlist_edit_data.mode, PlaylistEditMode::Reorder, playlist_edit_data_to_str(&PlaylistEditMode::Reorder));
									ui.selectable_value(&mut playlist_edit_data.mode, PlaylistEditMode::Remove, playlist_edit_data_to_str(&PlaylistEditMode::Remove));
							});
							match playlist_edit_data.mode {
								PlaylistEditMode::AddRemove => {
									self.browse_mode = LeftPanelMode::SelectSongs;
								},
								PlaylistEditMode::Reorder => {
									self.browse_mode = LeftPanelMode::ReorderSongs;
								},
								PlaylistEditMode::Remove => {
									self.browse_mode = LeftPanelMode::RemoveSongs;
								},
							}
							if prev != playlist_edit_data.mode {
								match playlist_edit_data.mode {
									/* Shouldn't need to do anything here */
									PlaylistEditMode::AddRemove => {
										let mut new_vec: Vec<String> = Vec::<String>::new();
										for song in &playlist_edit_data.edit_vec {
											if !playlist_edit_data.removal_map.contains_key(song) {
												playlist_edit_data.removal_map.remove(song);
												new_vec.push(song.clone());
											} else {
												playlist_edit_data.edit_map.remove(song);
											}
										}
										playlist_edit_data.edit_vec = new_vec;
										playlist_edit_data.removal_map = HashMap::<String, PlaylistElementType>::new();
									},
									PlaylistEditMode::Reorder => {
										let mut new_vec: Vec<String> = Vec::<String>::new();
										for song in &playlist_edit_data.edit_vec {
											if !playlist_edit_data.removal_map.contains_key(song) {
												playlist_edit_data.removal_map.remove(song);
												new_vec.push(song.clone());
											} else {
												playlist_edit_data.edit_map.remove(song);
											}
										}
										playlist_edit_data.edit_vec = new_vec;
										playlist_edit_data.removal_map = HashMap::<String, PlaylistElementType>::new();
									},
									PlaylistEditMode::Remove => {
										
									},
								}
							}
						});

						if remove {
							if let Some(pl) = self.persistent_data.playlists.get(playlist_edit_data.playlist_index) && pl.songs.len() == 0 {
								self.persistent_data.playlists.remove(playlist_edit_data.playlist_index);
							} else {
								self.active_playlist_index = Some(playlist_edit_data.playlist_index);
							}
							self.browse_mode = LeftPanelMode::Playlists;
							self.playlist_tree = None; // Force reload
							self.edit_playlist_data = None;

							if self.persistent_data.data_file_exists {
								let write_to = build_full_filepath(&self.installed_location, "internal_pinetree_data.txt");
								/* TODO: Display error message on failure */
								if let Ok(_) = write_internal_data(&write_to, &self.persistent_data) {
									
								} else {
									println!("Error in saving");
								}
							}
						} else {
							let row_count = playlist_edit_data.edit_vec.len();
							egui::ScrollArea::vertical().show_rows(ui, 16.0, row_count, |ui, row_range| {
								for row in row_range {
									if let Some(song) = playlist_edit_data.edit_vec.get(row) {
										ui.horizontal(|ui| {
											let mut checked = !playlist_edit_data.removal_map.contains_key(song);
											if ui.checkbox(&mut checked, "").clicked() {
												if checked {
													playlist_edit_data.removal_map.remove(song);
												} else {
													playlist_edit_data.removal_map.insert(song.clone(), PlaylistElementType::Song);
												}
											}
											if checked {
												ui.label(extract_file_name(song));
											} else {
												ui.disable();
												ui.label(extract_file_name(song));
											}
										});
									}
								}
							});
						}


					} else {
						unreachable!();
					}
				},
				LeftPanelMode::ReorderSongs => {
					if let Some(playlist_edit_data) = &mut self.edit_playlist_data {
						let mut remove = false;
						ui.add(egui::TextEdit::singleline(&mut playlist_edit_data.playlist_name).hint_text("Playlist name..."));
						ui.horizontal(|ui| {
							if ui.button("Cancel").clicked() {
								remove = true;
							}
							/*
							* When in add mode: Only push to hashmap
							* When in reorder mode: First rebuild the vec from the hash map, then only do reordering.
							*/
							ui.add_space(2.5);
							if ui.button("Save").clicked() {
								if let Some(pl) = self.persistent_data.playlists.get_mut(playlist_edit_data.playlist_index) {
									pl.is_open = true;
									pl.name = playlist_edit_data.playlist_name.clone();

									pl.songs = rebuild_ordered_vec(&mut playlist_edit_data.edit_vec, &mut playlist_edit_data.edit_map);
								}
								remove = true;
							}
							let prev =  playlist_edit_data.mode.clone();
							egui::ComboBox::from_label(" ")/* Lmao */
								.selected_text(
									playlist_edit_data_to_str(&playlist_edit_data.mode)
								)
								.show_ui(ui, |ui| {
									ui.selectable_value(&mut playlist_edit_data.mode, PlaylistEditMode::AddRemove, playlist_edit_data_to_str(&PlaylistEditMode::AddRemove));
									ui.selectable_value(&mut playlist_edit_data.mode, PlaylistEditMode::Reorder, playlist_edit_data_to_str(&PlaylistEditMode::Reorder));
									ui.selectable_value(&mut playlist_edit_data.mode, PlaylistEditMode::Remove, playlist_edit_data_to_str(&PlaylistEditMode::Remove));
							});
							match playlist_edit_data.mode {
								PlaylistEditMode::AddRemove => {
									self.browse_mode = LeftPanelMode::SelectSongs;
								},
								PlaylistEditMode::Reorder => {
									self.browse_mode = LeftPanelMode::ReorderSongs;
								},
								PlaylistEditMode::Remove => {
									self.browse_mode = LeftPanelMode::RemoveSongs;
								},
							}
							if prev != playlist_edit_data.mode {
								match playlist_edit_data.mode {
									/* Shouldn't need to do anything here */
									PlaylistEditMode::AddRemove => {},
									PlaylistEditMode::Reorder => {
										playlist_edit_data.edit_vec = rebuild_ordered_vec(&mut playlist_edit_data.edit_vec, &mut playlist_edit_data.edit_map);
									},
									PlaylistEditMode::Remove => {
										playlist_edit_data.edit_vec = rebuild_ordered_vec(&mut playlist_edit_data.edit_vec, &mut playlist_edit_data.edit_map);

									},
								}
							}
						});
						ui.add_space(5.0);
						file_action = render_playlist_reordering(ui, &audio_data.song_name, playlist_edit_data);

						if remove {
							if let Some(pl) = self.persistent_data.playlists.get(playlist_edit_data.playlist_index) && pl.songs.len() == 0 {
								self.persistent_data.playlists.remove(playlist_edit_data.playlist_index);
							} else {
								self.active_playlist_index = Some(playlist_edit_data.playlist_index);
							}
							self.browse_mode = LeftPanelMode::Playlists;
							self.playlist_tree = None; // Force reload
							self.edit_playlist_data = None;

							if self.persistent_data.data_file_exists {
								let write_to = build_full_filepath(&self.installed_location, "internal_pinetree_data.txt");
								if let Ok(_) = write_internal_data(&write_to, &self.persistent_data) {
									
								} else {
									println!("Error in saving");
								}
							}
						}
					} else {
						unreachable!();
					}
				},
			}
		});

		match file_action {
			FileActions::None => {},
			FileActions::OpenDirectory(dir) => {
				init_directory_at_filepath(&dir, &mut self.directory_map);
				ctx.request_repaint();
				self.active_search_text = "".to_string();
				self.directory_tree = None;
				self.searched_directory_tree = None;
			},
			FileActions::OpenDirectoryRecursive(dir) => {
				init_directory_at_filepath_recursive(&dir, &mut self.directory_map);
				ctx.request_repaint();
				self.active_search_text = "".to_string();
				self.directory_tree = None;
				self.searched_directory_tree = None;
			},
			FileActions::CloseDirectory(dir) => {
				self.directory_map.remove(&dir);
				self.active_search_text = "".to_string();
				self.directory_tree = None;
				self.searched_directory_tree = None;
			},
			FileActions::EnterDirectory(dir) => {
				self.search_text = "".to_string();
				self.active_search_text = "".to_string();
				self.active_directory_filepath = dir.clone();
				self.current_song_folder = dir;
				self.directory_tree = None;
				self.searched_directory_tree = None;
			},
			FileActions::OpenPlaylist(index) => {
				if let Some(playlist) = self.persistent_data.playlists.get_mut(index) {
					playlist.is_open = true;
					self.playlist_tree = None;
					self.active_search_text_playlists = "".to_string();
					ctx.request_repaint();
				}
			},
			FileActions::ClosePlaylist(index) => {
				if let Some(playlist) = self.persistent_data.playlists.get_mut(index) {
					playlist.is_open = false;
					self.playlist_tree = None;
					self.active_search_text_playlists = "".to_string();
					ctx.request_repaint();
				}
			},
			FileActions::EnterPlaylist(index) => {
				self.search_text = "".to_string();
				self.active_search_text = "".to_string();
				self.active_playlist_index = Some(index);
				self.playlist_tree = None;
				ctx.request_repaint();
			},
			FileActions::PlaySong(song) => {
				send_audio_signal(&self.audio_message_channel, MessageToAudio::PlaySong(song));
			},
			FileActions::AddSongToPlaylist(index) => {
				if let Some(directory_tree_elements) = &self.directory_tree && let Some(playlist_edit_data) = &mut self.edit_playlist_data {
					if let Some(song) = directory_tree_elements.get(index) {
						playlist_edit_data.last_touched_index = index;
						if playlist_edit_data.edit_map.contains_key(&song.name) {
							playlist_edit_data.edit_map.remove(&song.name);
						} else {
							playlist_edit_data.edit_map.insert(song.name.clone(), PlaylistElementType::Song);
						}
					}
				}
			},
			FileActions::AddSongToPlaylistStrong(index) => {
				if let Some(directory_tree_elements) = &self.directory_tree && let Some(playlist_edit_data) = &mut self.edit_playlist_data {
					let add = if let Some(song) = directory_tree_elements.get(index) && playlist_edit_data.edit_map.contains_key(&song.name) {false} else {true};
					let start_index = std::cmp::min(index, playlist_edit_data.last_touched_index);
					let end_index = std::cmp::max(index, playlist_edit_data.last_touched_index);
					for i in start_index..=end_index {
						if let Some(element) = directory_tree_elements.get(i) && !element.is_dir {
							if add {
								playlist_edit_data.edit_map.insert(element.name.clone(), PlaylistElementType::Song);
							} else {
								playlist_edit_data.edit_map.remove(&element.name);
							}
						}
					}
				}
			},
		}
		
		if request_refresh {
			ctx.request_repaint();
			// active_directory = None;
			self.active_playlist_index = None;
			self.playlist_tree = None;
			self.searched_directory_tree = None;
			self.directory_tree = None;
			self.active_search_text = "".to_string();
			self.active_search_text_playlists = "".to_string();
			self.directory_map.clear();
		}

		egui::CentralPanel::default().show(ctx, |ui| {
			ui.horizontal(|ui| {
				let player_text = if self.central_panel_mode == CentralPanelMode::PlayerMode {
					egui::RichText::new("Player").underline().strong()
				} else {
					egui::RichText::new("Player")
				};
				
				let settings_text = if self.central_panel_mode == CentralPanelMode::Settings {
					egui::RichText::new("Settings").underline().strong()
				} else {
					egui::RichText::new("Settings")
				};

				let about_text = if self.central_panel_mode == CentralPanelMode::About {
					egui::RichText::new("About").underline().strong()
				} else {
					egui::RichText::new("About")
				};
				
				
				if ui.button(player_text).clicked() {
					self.central_panel_mode = CentralPanelMode::PlayerMode;
				}
				if ui.button(settings_text).clicked() {
					self.central_panel_mode = CentralPanelMode::Settings;
				}
				if ui.button(about_text).clicked() {
					self.central_panel_mode = CentralPanelMode::About;
				}
			});
			match self.central_panel_mode {
				CentralPanelMode::PlayerMode => {
					ui.vertical_centered(|ui| {
						ui.heading("Player Parameters");
						ui.add_space(5.0);
					});
					ui.horizontal(|ui| {
						ui.label("Speed: ");
						// Note to self: Fill color can be adjusted with ui.visuals_mut().selection.bg_fill = Color32::{INSERT COLOR HERE};
						let speed_slider = ui.add(
							egui::Slider::new(&mut self.song_speed, 0.5..=2.0)
							.handle_shape(egui::style::HandleShape::Rect { aspect_ratio: 1.0 })
							.show_value(false)
							.trailing_fill(true)
							.logarithmic(true)
						);
						if speed_slider.dragged() {
							send_audio_signal(&self.audio_message_channel, MessageToAudio::UpdateSpeed(self.song_speed));
						}
						if ui.button("Reset").clicked() {
							self.song_speed = 1.0;
							send_audio_signal(&self.audio_message_channel, MessageToAudio::UpdateSpeed(self.song_speed));
						}
					});

					ui.vertical_centered(|ui| {
						ui.add_space(5.0);
						ui.heading("Song Info");
						ui.add_space(5.0);
						ui.label("Currently unimplemented (coming in final release)");
					});
				},
				CentralPanelMode::Settings => {
					ui.vertical_centered(|ui| {
						ui.heading("Settings");
						ui.add_space(5.0);
					});
					ui.horizontal(|ui| {
						ui.label("Audio Device: ");
						egui::ComboBox::from_label("")
							.selected_text("Default (unimplemented)")
							.show_ui(ui, |ui| {
								ui.selectable_value(&mut self.browse_mode, LeftPanelMode::Files, "Default (unimplemented)");
							}
						);
					});
					ui.horizontal(|ui| {
						let current_theme = self.persistent_data.theme.clone();

						ui.label("Theme: ");
						egui::ComboBox::from_label(" ")/* Lmao */
							.selected_text(
								if self.persistent_data.theme == ThemePref::DARK {
									"Dark (default)".to_string()
								} else {
									theme_to_str(&self.persistent_data.theme)
								}
							)
							.show_ui(ui, |ui| {
								ui.selectable_value(&mut self.persistent_data.theme, ThemePref::DARK, "Dark (default)");
								ui.selectable_value(&mut self.persistent_data.theme, ThemePref::LIGHT, "Light");
							}
						);
						if current_theme != self.persistent_data.theme {
							set_visuals(ctx, &self.persistent_data.theme);
						}
					});
					ui.horizontal(|ui| {
						ui.label("Default on-finish:");
						egui::ComboBox::from_label("  ")
							.selected_text(loop_behavior_to_str(&self.persistent_data.default_on_finish))
							.show_ui(ui, |ui| {
								ui.selectable_value(&mut self.persistent_data.default_on_finish, LoopBehavior::Stop, "Stop");
								ui.selectable_value(&mut self.persistent_data.default_on_finish, LoopBehavior::Loop, "Loop");
								ui.selectable_value(&mut self.persistent_data.default_on_finish, LoopBehavior::Shuffle, "Shuffle");
								ui.selectable_value(&mut self.persistent_data.default_on_finish, LoopBehavior::Next, "Next");
							}
						);
					});
					ui.horizontal(|ui| {
						ui.label("Previous Song Behavior:");
						egui::ComboBox::from_label("   ")
							.selected_text(
								match self.persistent_data.prev_behavior {
									PrevBehavior::Above => {
										"Go Up Song List"
									},
									PrevBehavior::History => {
										"Previously Listened"
									}
								}
							)
							.show_ui(ui, |ui| {
								ui.selectable_value(&mut self.persistent_data.prev_behavior, PrevBehavior::Above, "Go Up Song List");
								ui.selectable_value(&mut self.persistent_data.prev_behavior, PrevBehavior::History, "Previously Listened");
							}
						);
						if self.persistent_data.prev_behavior != self.prev_behavior {
							self.prev_behavior = self.persistent_data.prev_behavior;
							send_audio_signal(&self.audio_message_channel, MessageToAudio::UpdatePrevBehavior(self.prev_behavior));
						}
					});
					ui.horizontal(|ui| {
						if !self.hide_fp {
							ui.label("Default Folder: ");
							ui.text_edit_singleline(&mut self.persistent_data.default_directory);
						} else {
							ui.label("Default Folder: ");
							let mut hidden = "(Hidden)";
							ui.horizontal(|ui| {
								ui.disable();
								ui.text_edit_singleline(&mut hidden);
							});
						}
					});
					ui.horizontal(|ui| {
						ui.label("Hide File Paths by Default: ");
						ui.checkbox(&mut self.persistent_data.hide_directories_by_default, "");
					});
					ui.horizontal(|ui| {
						ui.label("Default volume: ");
						ui.add_sized([120.0, ui.spacing().interact_size.y],
							egui::Slider::new(&mut self.persistent_data.default_volume, -0.2..=1.0)
							.show_value(false)
							.trailing_fill(true)
						);
						if ui.button("Match").clicked() {
							self.persistent_data.default_volume = self.song_volume;
						}
					});
					ui.horizontal(|ui| {
						ui.label("Shuffle memory: ");
						let audio_thread_sm = audio_data.shuffle_memory;

						let response = ui.add(egui::TextEdit::singleline(&mut self.shuffle_memory_text).desired_width(24.0));
						self.shuffle_memory_text.retain(|c| c.is_ascii_digit());
						if response.lost_focus() && self.shuffle_memory_text.len() == 0 {
							self.shuffle_memory_text = format!("0");
						}
						if let Ok(new_value) = self.shuffle_memory_text.parse::<usize>() {
							self.persistent_data.shuffle_memory = new_value;
							self.shuffle_memory = new_value;
							if new_value != audio_thread_sm {
								send_audio_signal(&self.audio_message_channel, MessageToAudio::UpdateShuffleMemory(self.shuffle_memory));
							}
						}
					});
					#[cfg(target_os = "windows")] {
						ui.horizontal(|ui| {
							use windows_sys::Win32::Foundation::HWND;
							use raw_window_handle::{HasWindowHandle, RawWindowHandle};
							
							ui.label("Pin window to top: ");
							if ui.checkbox(&mut self.pinned_mode, "").clicked() {
								if let Ok(handle) = frame.window_handle() {
									match handle.as_raw() {
										RawWindowHandle::Win32(handle) => {
											let hwnd = handle.hwnd.get() as HWND;
											set_pin_mode(hwnd, self.pinned_mode);
										},
										_ => {},
									};
								}
							}
						});
					}
					#[cfg(target_family = "unix")] {
						ui.horizontal(|ui| {
							ui.disable();
							
							ui.label("Pin window to top: ");
							if ui.checkbox(&mut self.pinned_mode, "").clicked() {}
						}).response.on_hover_text_at_pointer("This option is not supported on Linux - certain window managers do not support this.");
					}
					
					if ui.button("Save").clicked() {
						/* TODO: Error handling */
						let write_to = build_full_filepath(&self.installed_location, "internal_pinetree_data.txt");
						if let Ok(_) = write_internal_data(&write_to, &self.persistent_data) {
							self.save_err = SaveError::Success;
						} else {
							self.save_err = SaveError::Error(format!("Error: Failed to save internal data"))
						}
					}
					match &mut self.save_err {
						SaveError::None => {},
						SaveError::Success => {
							ui.horizontal(|ui| {
								if ui.button("X").clicked() {
									self.save_err = SaveError::None;
								}
								ui.label("Successfully saved settings");
							});
						},
						SaveError::Error(string) => {
							let mut clear = false;
							ui.horizontal(|ui| {
								if ui.button(egui::RichText::new("X").color(egui::Color32::RED)).clicked() {
									clear = true;
								} else {
									ui.label(egui::RichText::new(string.clone()).color(egui::Color32::RED));
								}
							});
							if clear {
								self.save_err = SaveError::None;
							}
						},
					}
				},
				CentralPanelMode::Installer => {
					ui.vertical_centered(|ui| {
						ui.heading("Welcome to Pinetree!");
						ui.add_space(5.0);
					});
					ui.label("Pinetree appears to have no saved data associated with it currently.");
					ui.add_space(5.0);
					ui.label("If you would like to install Pinetree, please see the settings below. Otherwise, it is perfectly useable as just a plain executable - give it a song folder and start listening!");
					ui.add_space(20.0);
					ui.horizontal(|ui| {
						ui.label("Install location: ");
						ui.text_edit_singleline(&mut self.installer_data.install_path);
					});
					ui.horizontal(|ui| {
						ui.label("Default song folder: ");
						ui.text_edit_singleline(&mut self.installer_data.default_song_folder);
					});

					ui.add_space(20.0);
					
					ui.vertical_centered(|ui| {
						if ui.button(egui::RichText::new("Install").size(16.0).strong()).clicked() {
							self.persistent_data.default_directory = self.installer_data.default_song_folder.clone();
							/* TODO: Add proper error display */
							let install_error = run_install(&self.installer_data, &self.persistent_data);

							if let Some(install_error) = install_error {
								self.installer_error = Some(install_error);
							} else {
								self.central_panel_mode = CentralPanelMode::InstallationSuccess;
								self.installed_location = self.installer_data.install_path.clone();
								self.active_directory_filepath = self.installer_data.default_song_folder.clone();
								self.current_song_folder = self.installer_data.default_song_folder.clone();
								ctx.request_repaint();
							}
						}
					});

					ui.add_space(20.0);
					if let Some(err) = &self.installer_error {
						let err_text = egui::RichText::new(err).color(egui::Color32::RED);
						ui.horizontal(|ui| {
							if ui.button(egui::RichText::new("X").color(egui::Color32::RED)).clicked() {
								self.installer_error = None;
							} else {
								ui.label(err_text);
							}
						});
					}

					ui.label("Additional note: Creating shortcuts/taskbar icons automatically is unfortunately not supported because it would mean requiring admin permissions on Windows");
				},
				CentralPanelMode::About => {
					egui::ScrollArea::vertical().show(ui, |ui| {
						ui.vertical_centered(|ui| {
							ui.heading("About");
							ui.add_space(5.0);
						});
						ui.label("The download page and user manual can be found here:");
						ui.hyperlink("https://katelyndoucette.com/projects/pinetree");
	
						ui.add_space(10.0);
						ui.label("This application was written by Katelyn Doucette.");
						ui.hyperlink("https://katelyndoucette.com/");
						ui.add_space(10.0);
						ui.label("It is also free and open source. The source code can be found at the repository here:");
						ui.hyperlink("https://github.com/Laturas/Pinetree");
						ui.vertical_centered(|ui| {
							ui.heading("Reference Manual");
							ui.add_space(5.0);
						});
						ui.label("The following are some keyboard shortcuts you can use to navigate around:");
						ui.add_space(5.0);
						ui.label("- Ctrl + P: Pauses or unpauses the song");
						ui.label("- Ctrl + F: Search in current directory/playlist");
						ui.label("- Ctrl + R: Refreshes current directory/playlist");
						ui.label("- LeftArrow/RightArrow: Skips behind/forward 5 seconds in the current song");
						ui.label("- Ctrl + LeftArrow/RightArrow: Plays the previous song or skips to the next song");
	
						ui.add_space(10.0);
						ui.label("Thank you for using Pinetree!");
					});
				},
				CentralPanelMode::InstallationSuccess => {
					ui.vertical_centered(|ui| {
						ui.heading("Installation success");
						ui.add_space(5.0);
					});
					ui.label("Pinetree was successfully installed");
					if ui.button("Ok").clicked() {
						self.central_panel_mode = CentralPanelMode::Settings;
					}
				},
			}
		});
	}
}

fn main() -> eframe::Result {
	let img = eframe::icon_data::from_png_bytes(include_bytes!("./../resources/Pinetree_Logo.png"));
	let options = if let Ok(img) = img {
		eframe::NativeOptions {
			viewport: egui::ViewportBuilder::default()
				.with_inner_size([800.0, 600.0]).with_icon(img),
			..Default::default()
		}
	} else {
		eframe::NativeOptions {
			viewport: egui::ViewportBuilder::default()
				.with_inner_size([800.0, 600.0]),
			..Default::default()
		}
	};
	eframe::run_native(
		"Pinetree Music Player",
		options,
		Box::new(|_cc| Ok(Box::new(MyApp::default()))),
	)
}
