#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use std::sync::{Arc, Mutex, Condvar};
use std::thread;
use rodio;
use std::collections::HashMap;

#[derive(PartialEq)]
enum LoopBehavior {
	Stop,
	Loop,
	Shuffle,
	Next,
}

#[derive(PartialEq)]
enum SongBrowseMode {
	Files,
	Playlists,
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

struct Playlist {
	name: String,
	songs: Vec<String>,
	is_open: bool,
}

struct EditPlaylistData {
	editing_playlist_index: usize,
	edit_map: HashMap<String, usize>,
}

struct MyApp {
	// For setting sizing and such
	first_frame_rendered: bool,

	loop_behavior: LoopBehavior,
	browse_mode: SongBrowseMode,

	current_song_folder: String,
	current_song_name: String,

	search_text: String,
	advanced_search_active: bool,
	genre_search_text: String,
	artist_search_text: String,
	
	song_speed: f32,
	song_reverb: f32,
	song_volume: f32,
	// TODO: Derive this from rodio?
	// Principle of least duplication
	song_playback_position_secs: u64,

	active_directory_filepath: String,

	directory_map: HashMap<String, Directory>,
	song_map: HashMap<String, Song>,

	directory_tree: Option<Vec<DirTreeElement>>,
	playlist_tree: Option<Vec<PlaylistTreeElement>>,

	active_search_text: String,
	searched_directory_tree: Option<Vec<usize>>,

	playlists: Vec<Playlist>,
	active_playlist_index: Option<usize>,

	active_search_text_playlists: String, 
	searched_playlist_tree: Option<Vec<usize>>,

	edit_playlist_data: Option<EditPlaylistData>,

	/* TODO: Replace with stacks to prevent missed messages */
	audio_message_channel: Arc<(Mutex<Vec<MessageToAudio>>, Condvar)>,
	audio_receive_channel: Arc<(Mutex<Vec<MessageToGui>>, Condvar)>,
}

#[derive(Clone)]
struct RodioData {
	playback_position: usize,
	song_length: usize,
	is_paused: bool,
	song_name: String,
}

enum MessageToGui {
	None,
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
	reverb: f32,
	end_behavior: LoopBehavior,
	test: usize,
}

// struct DirTreeElement {
// 	name: String,
// 	depth: usize,
// 	is_dir: bool,
// 	/* For directories only, indicating whether it is active or not */
// 	is_active: bool,
// }

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

use std::io::BufRead;

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
fn init_playlist() -> Vec<Playlist> {
	let file = std::fs::File::open("./internal_pinetree_data.dat");
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
					playlist_name = Some((&line[9..]).to_string());
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
use std::time::Duration;

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

fn audio_thread_play_song(file_path: &str, sink: &mut rodio::Sink, recieve_pair: &Arc<(Mutex<Vec<MessageToAudio>>, Condvar)>) -> Option<usize> {
	sink.clear();
	let mut return_value = None;
	if let Ok(file) = std::fs::File::open(&file_path) {
		let reader = std::io::BufReader::<std::fs::File>::new(file);
		
		let _ = sink.try_seek(std::time::Duration::from_millis(0));
		if let Ok(elem) = rodio::Decoder::new_mp3(reader) {
			if let Some(len) = elem.total_duration() {
				let rodio_pair = Arc::clone(recieve_pair);
				sink.append(elem);
				sink.append(EndCallback {
					callback: Some(Box::new(move || {
						song_end_callback(rodio_pair);
					})),
				});

				return_value = Some(len.as_millis() as usize)
			} else if let Ok(len) = mp3_duration::from_path(&file_path) {
				let rodio_pair = Arc::clone(recieve_pair);
				sink.append(elem);
				sink.append(EndCallback {
					callback: Some(Box::new(move || {
						song_end_callback(rodio_pair);
					})),
				});

				return_value = Some(len.as_millis() as usize)
			}
		}
	} 
	sink.play();
	return return_value;
}

pub const DEFAULT_VOLUME: f32 = 0.5;
pub const DEFAULT_REVERB: f32 = 0.0;
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

/* TODO: God damnit rodio */
use mp3_duration;

fn audio_thread_loop(recieve_pair: Arc<(Mutex<Vec<MessageToAudio>>, Condvar)>, send_pair: Arc<(Mutex<Vec<MessageToGui>>, Condvar)>) {
	let (output_stream, audio_sink) = rodio::OutputStream::try_default().unwrap();
	let mut song_path = "".to_string();
	let mut song_index = 0;
	let mut song_length = 0;
	let mut audio_thread_data = AudioThreadData {
		// This has to exist even if unused, otherwise the lifetime causes the program to crash
		_stream: output_stream,
		sink: rodio::Sink::try_new(&audio_sink).unwrap(),
		volume: DEFAULT_VOLUME,
		speed: DEFAULT_SPEED,
		reverb: DEFAULT_REVERB,
		end_behavior: LoopBehavior::Stop,
		test: 0,
	};
	audio_thread_data.sink.set_volume(audio_thread_data.volume);
	let lock = &recieve_pair.0;
	let cvar = &recieve_pair.1;
	let mut data_vec = lock.lock().unwrap();
	let mut current_songs_collection = Vec::<String>::new();
	loop {
		// If this unwrap fails, it should crash.
		data_vec = cvar.wait(data_vec).unwrap();
		while data_vec.len() > 0 {
			if let Some(data) = data_vec.get(data_vec.len() - 1) {
				match data {
					MessageToAudio::None => {println!("Do nothing");},
					MessageToAudio::PlaySong(song) => {
						song_path = song.clone();
						song_length = if let Some(len) = audio_thread_play_song(&song, &mut audio_thread_data.sink, &recieve_pair) {
							len
						}
						else {0};
					},
					/* TODO: Make this do something */
					MessageToAudio::UpdateEndBehavior(loop_behavior) => {
						audio_thread_data.end_behavior = clone_loop_behavior(&loop_behavior);
					},
					MessageToAudio::UpdateVolume(volume) => {
						audio_thread_data.volume = *volume;
						audio_thread_data.sink.set_volume(audio_thread_data.volume);
					},
					MessageToAudio::RequestRodioData => {
						let send_lock = &send_pair.0;
						let send_cvar = &send_pair.1;
						let mut response_vec = send_lock.lock().unwrap();
						response_vec.push(MessageToGui::Data(RodioData {
							song_length: song_length,
							playback_position: audio_thread_data.sink.get_pos().as_millis() as usize,
							is_paused: audio_thread_data.sink.is_paused(),
							song_name: song_path.clone(),
						}));
						send_cvar.notify_one();
					},
					MessageToAudio::Seek(position) => {
						let _ = audio_thread_data.sink.try_seek(std::time::Duration::from_millis(*position));
					},
					MessageToAudio::TogglePause => {
						if audio_thread_data.sink.is_paused() {
							audio_thread_data.sink.play();
						} else {
							audio_thread_data.sink.pause();
						}
					},
					MessageToAudio::SongEnd => {
						audio_thread_data.sink.clear();
						match audio_thread_data.end_behavior {
							LoopBehavior::Stop => {
								song_path = "".to_string();
							},
							LoopBehavior::Loop => {
								song_length = if let Some(len) = audio_thread_play_song(&song_path, &mut audio_thread_data.sink, &recieve_pair) {
									len
								}
								else {0};
							},
							/* TODO: */
							LoopBehavior::Next => {
								if current_songs_collection.len() > 0 {
									if let Some(song) = current_songs_collection.get((song_index + 1) % current_songs_collection.len() as usize) {
										song_path = song.to_string();
										song_length = if let Some(len) = audio_thread_play_song(&song_path, &mut audio_thread_data.sink, &recieve_pair) {
											len
										}
										else {0};
									}
								}
							}
							/* TODO: */
							LoopBehavior::Shuffle => {
								println!("Unimplemented");
							}
						}
					},
					MessageToAudio::SetSongCollection(vec) => {
						current_songs_collection.clear();
						let mut i = 0;
						let mut found = false;
						for song in vec {
							if !found && *song == *song_path {
								song_index = i;
								found = true;
							}
							i += 1;
							/* TODO: Ughhh */
							current_songs_collection.push(song.clone());
						}
					}
				}
				data_vec.pop();
			}
		}
	}
}

fn request_rodio_data(send_pair: &Arc<(Mutex<Vec<MessageToAudio>>, Condvar)>, recv_pair: &Arc<(Mutex<Vec<MessageToGui>>, Condvar)>) -> RodioData {
	send_audio_signal(send_pair, MessageToAudio::RequestRodioData);
	let lock = &recv_pair.0;
	let cvar = &recv_pair.1;
	if let Ok(mut vec) = lock.lock() {
		loop {
			while vec.len() == 0 {
				vec = cvar.wait(vec).unwrap();
			}
			if let Some(element) = vec.get(0) {
				match element {
					MessageToGui::None => {
						vec.pop();
					},
					MessageToGui::Data(data) => {
						let ret = data.clone();
						vec.pop();
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
	None,
	PlaySong(String),
	UpdateEndBehavior(LoopBehavior),
	UpdateVolume(f32),
	RequestRodioData,
	Seek(u64),
	TogglePause,
	SongEnd,
	SetSongCollection(Vec<String>),
}

impl Default for MyApp {
	fn default() -> Self {
		let mut dir_map = HashMap::<String, Directory>::new();
		init_directory_at_filepath("/home/katelyn/Music/songs", &mut dir_map);

		let message_param: Vec<MessageToAudio> = Vec::<MessageToAudio>::new();
		let gui_message_param: Vec<MessageToGui> = Vec::<MessageToGui>::new();

		let audio_thread_recieve = Arc::new((Mutex::new(message_param), Condvar::new()));
		let gui_thread_send = Arc::clone(&audio_thread_recieve);
		
		let audio_thread_send = Arc::new((Mutex::new(gui_message_param), Condvar::new()));
		let gui_thread_recieve = Arc::clone(&audio_thread_send);

		thread::spawn(move || {
			audio_thread_loop(audio_thread_recieve, audio_thread_send);
		});

		Self {
			first_frame_rendered: false,
			loop_behavior: LoopBehavior::Stop,
			browse_mode: SongBrowseMode::Files,
			current_song_folder: "/home/katelyn/Music/songs".to_string(),
			song_speed: DEFAULT_SPEED,
			song_reverb: DEFAULT_REVERB,
			search_text: "".to_string(),
			genre_search_text: "".to_string(),
			artist_search_text: "".to_string(),
			current_song_name: "".to_string(),
			advanced_search_active: false,
			song_volume: DEFAULT_VOLUME,
			song_playback_position_secs: 0,
			// songs_list: song_entry_list,
			active_directory_filepath: "/home/katelyn/Music/songs".to_string(),
			directory_map: dir_map,
			song_map: HashMap::<String, Song>::new(),
			directory_tree: None,
			playlist_tree: None,

			active_search_text: "".to_string(),
			searched_directory_tree: None,
			searched_playlist_tree: None,
			
			playlists: init_playlist(),
			active_playlist_index: None,
			
			active_search_text_playlists: "".to_string(),

			edit_playlist_data: None,

			audio_message_channel: gui_thread_send,
			audio_receive_channel: gui_thread_recieve,
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

/**
* TODO: Handle windows needing to be a special snowflake
*/
fn build_full_filepath(input_first_half: &str, input_second_half: &str) -> String {
	let mergeable_first_half = trim_slash_if_exists(input_first_half);
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
		// TODO: make case-insensitive sort function.
		songs_vec.sort();
		subdirectory_vec.sort();

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

fn song_browse_mode_to_str(song_browse_mode: &SongBrowseMode) -> &'static str {
	match song_browse_mode {
		SongBrowseMode::Files => "Files",
		SongBrowseMode::Playlists => "Playlists",
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

#[derive(PartialEq)]
enum FileActions {
	None,
	OpenDirectory(String),
	CloseDirectory(String),
	EnterDirectory(String),
	PlaySong(String),

	OpenPlaylist(usize),
	ClosePlaylist(usize),
	EnterPlaylist(usize),
}

fn render_song_entry_ui_element(ui: &mut egui::Ui, current_song: &str, is_current_song: bool, depth: usize) -> FileActions {
	let mut return_value = FileActions::None;
	ui.horizontal(|ui| {
		ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);

		/* Indenting */
		for _ in 0..depth {ui.add_space(10.0);}

		if ui.button("▶").clicked() {
			return_value = FileActions::PlaySong(current_song.to_string());
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
				return_value = FileActions::OpenDirectory(current_directory.to_string());
			}
		}
		ui.style_mut().visuals.hyperlink_color = egui::Color32::from_rgb(180, 180, 255);
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

/**
* TODO: Cache this result somehow
*/
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
pub const SEARCH: egui::KeyboardShortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::F);

fn init_edit_playlist_data(index: usize, playlists: &Vec<Playlist>) -> Option<EditPlaylistData> {
	if let Some(playlist) = playlists.get(index) {
		let mut map = HashMap::<String, usize>::new();
		let mut i = 0;
		for song in &playlist.songs {
			map.insert(song.to_string(), i);
			i += 1;
		}
		return Some(EditPlaylistData {
			editing_playlist_index: index,
			edit_map: map,
		});
	} else {
		return None;
	}
}

fn render_directory_elements(ui: &mut egui::Ui, directory_tree_vec: &Option<Vec<DirTreeElement>>, searched_vec: &Option<Vec<usize>>, active_song_name: &str) -> FileActions {
	let mut file_action = FileActions::None;
	if let Some(directory_tree_elements) = directory_tree_vec {
		egui::ScrollArea::vertical().show_rows(ui, 16.0, directory_tree_elements.len(), |ui, row_range| {
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
						render_song_entry_ui_element(ui, &element.name, is_active_song, element.depth)
					};
					match re_code {
						FileActions::None => {},
						FileActions::OpenDirectory(dir) => {
							file_action = FileActions::OpenDirectory(dir.clone());
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
						FileActions::OpenPlaylist(_) | FileActions::ClosePlaylist(_) | FileActions::EnterPlaylist(_) => {},
					}
				}
			}
		});
	}
	return file_action;
}

/// Human hearing is logarithmic, so the volume slider follows an exponential curve to compensate.
/// Specifically this is one that is meant to very closely match the decibel scale (hence the magic number of 6.908).
/// 
/// Has to be clamped though unfortunately, because exponential curves never technically reach 0.
/// This creates a sharp cutoff at the lowest volume, but at that point it's quiet enough I don't imagine people will notice.
/// 
/// Surprised this isn't more common.
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
		egui::ScrollArea::vertical().show_rows(ui, 16.0, playlist_tree_elements.len(), |ui, row_range| {
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
						if let FileActions::PlaySong(song) = render_song_entry_ui_element(ui, &song_name, is_active_song, song_depth) {
							file_action = FileActions::PlaySong(song.clone());
						}
					} else {
						if let Some(pl) = playlists.get(element.playlist_position) {
							match render_directory_entry_ui_element(ui, &pl.name, pl.is_open, 0) {
								FileActions::None | FileActions::PlaySong(_) => {},
								FileActions::OpenDirectory(_) => {
									file_action = FileActions::OpenPlaylist(element.playlist_position);
								},
								FileActions::CloseDirectory(_) => {
									file_action = FileActions::ClosePlaylist(element.playlist_position);
								},
								FileActions::EnterDirectory(_) => {
									file_action = FileActions::EnterPlaylist(element.playlist_position);
								},
								FileActions::OpenPlaylist(_) | FileActions::ClosePlaylist(_) | FileActions::EnterPlaylist(_) => {},
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

impl eframe::App for MyApp {
	fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
		if !self.first_frame_rendered {
			ctx.set_visuals(egui::Visuals::dark());
			ctx.set_pixels_per_point(1.25); /* TODO: Figure this shit out */
			self.first_frame_rendered = true;
		}
		// 8 fps
		let audio_data = request_rodio_data(&mut self.audio_message_channel, &mut self.audio_receive_channel);
		ctx.request_repaint_after(std::time::Duration::from_millis(125));

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
					}
					let song_folder_field = ui.add(egui::TextEdit::singleline(&mut self.current_song_folder)
					.hint_text("Song folder..."))
					.on_hover_text("The file path of the current open folder.\nRelative to the file path of the executable");

					if song_folder_field.lost_focus() && song_folder_field.ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
						self.directory_tree = None;
						self.searched_directory_tree = None;
						self.active_directory_filepath = self.current_song_folder.clone();
					}
					
					ui.label("File path:");
				});
			});
			ui.add_space(5.0);
		});
		
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
				ui.label(if audio_data.song_name == "" {format!("No song playing")} else {format!("Now playing: {}", extract_file_name(&audio_data.song_name))})
			});
			ui.horizontal(|ui| {
				/* TODO: Implement Pause and Skip*/
				if ui.button(if audio_data.is_paused {format!("Unpause")} else {format!("Pause")}).clicked() {
					send_audio_signal(&self.audio_message_channel, MessageToAudio::TogglePause);
				}
				if ui.button("Skip").clicked() {
					send_audio_signal(&self.audio_message_channel, MessageToAudio::SongEnd);
				}

				self.song_playback_position_secs = (audio_data.playback_position) as u64;

				let secs = self.song_playback_position_secs / 1000;
				let prev_vol = self.song_volume;
				ui.label("Volume: ");
				ui.add_sized([120.0, ui.spacing().interact_size.y],
					egui::Slider::new(&mut self.song_volume, -0.2..=1.0)
					.show_value(false)
					.trailing_fill(true)
				);
				if prev_vol != self.song_volume {
					send_audio_signal(&self.audio_message_channel, MessageToAudio::UpdateVolume(volume_curve(self.song_volume)));
				}
				ui.label(format!("{}:{}{}", secs / 60, if secs % 60 < 10 {"0"} else {""}, secs % 60));
				let remaining_width = ui.available_width();
				ui.spacing_mut().slider_width = remaining_width;

				let seeker = ui.add_sized([remaining_width, ui.spacing().interact_size.y],
					egui::Slider::new(&mut self.song_playback_position_secs, 0..=(audio_data.song_length) as u64)
					.handle_shape(egui::style::HandleShape::Rect { aspect_ratio: 1.0 })
					.show_value(false)
					.trailing_fill(true)
					// Fill color can be adjusted with ui.visuals_mut().selection.bg_fill = Color32::{INSERT COLOR HERE};
				);

				if seeker.dragged() {
					send_audio_signal(&self.audio_message_channel, MessageToAudio::Seek(self.song_playback_position_secs));
				}
			});
		});

		let mut request_refresh = false;

		let mut file_action = FileActions::None;

		egui::SidePanel::left("left_panel").default_width(250.0).width_range(250.0..=650.0).show(ctx, |ui| {
			// LOL this has to be centered or else egui cannot resize the panel. This is so dumb lmao
			ui.vertical_centered(|ui| {
				ui.heading("Songs");
				ui.horizontal(|ui| {
					egui::ComboBox::from_label("")
						.selected_text(song_browse_mode_to_str(&self.browse_mode))
						.show_ui(ui, |ui| {
							ui.selectable_value(&mut self.browse_mode, SongBrowseMode::Files, "Files");
							ui.selectable_value(&mut self.browse_mode, SongBrowseMode::Playlists, "Playlists");
						}
					);
					if ui.button("Advanced").clicked() {
						self.advanced_search_active = !self.advanced_search_active;
					}
					request_refresh = ui.button("Refresh").clicked() || ctx.input_mut(|i| i.consume_shortcut(&REFRESH));
				});
			});
			ui.horizontal(|ui| {
				let response = ui.add(egui::TextEdit::singleline(&mut self.search_text)
					.hint_text("Search..."))
					.on_hover_text("Searches based on the file name");

				if ctx.input(|i| i.key_pressed(egui::Key::F) && i.modifiers.ctrl) {
					response.request_focus();
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
			ui.add_space(5.0);

			if self.browse_mode == SongBrowseMode::Files {
				if let Some(active_directory) = active_directory {
					/* Directory tree initialization in case it is null */
					if let None = self.directory_tree {
						let mut new_tree = Vec::<DirTreeElement>::new();
						let mut collection = Vec::<String>::new();
						get_dir_tree_elements(&mut new_tree, &active_directory.filepath_identifier, &self.directory_map, 0);

						for el in &new_tree {
							if !el.is_dir {
								collection.push(el.name.clone());
							}
						}
						send_audio_signal(&self.audio_message_channel, MessageToAudio::SetSongCollection(collection));
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
									&& element.name.to_lowercase().contains(&compare_to) {
										vec.push(i);
									}
								}
								Some(vec)
							};
						}
					}
					file_action = render_directory_elements(ui, &self.directory_tree, &self.searched_directory_tree, &audio_data.song_name);
				} else {
					ui.label("Error: Directory does not exist");
				}
			} else {
				let song_depth = if let Some(_) = self.active_playlist_index {0} else {1};
				if let None = self.playlist_tree {
					if let Some(active_playlist_index) = self.active_playlist_index {
						let mut tree = Vec::<PlaylistTreeElement>::new();
						let mut collection = Vec::<String>::new();
						if let Some(playlist) = self.playlists.get(active_playlist_index) {
							for song in &playlist.songs {
								tree.push(PlaylistTreeElement {
									song_name: Some(song.clone()),
									playlist_position: 0,
								});
								collection.push(song.to_string());
							}
							self.playlist_tree = Some(tree);
							send_audio_signal(&self.audio_message_channel, MessageToAudio::SetSongCollection(collection));
						} else {
							ui.label(format!("Unknown playlist error"));
							self.playlist_tree = None;
						}
					} else {
						self.playlist_tree = Some(build_playlist_tree(&self.playlists));
					}
				}

				if let Some(active_playlist_index) = self.active_playlist_index {
					ui.horizontal(|ui| {
						if ui.button("Edit songs").clicked() {
							request_refresh = true;
							self.edit_playlist_data = init_edit_playlist_data(active_playlist_index, &self.playlists);
						}
						if ui.button("Go back").clicked() {
							request_refresh = true;
						}
						if let Some(playlist) = self.playlists.get(active_playlist_index) {
							ui.label(egui::RichText::new(format!("{}", playlist.name)).strong());
						} else {
							ui.label(format!("Unknown playlist error"));
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
								if let Some(name) = &element.song_name && name.to_lowercase().contains(&compare_to) {
									tmp_vec.push(position);
								} else if let Some(playlist) = self.playlists.get(element.playlist_position)
								&& playlist.name.to_lowercase().contains(&compare_to) {
									tmp_vec.push(position);
								}
								position += 1;
							}
							self.searched_playlist_tree = Some(tmp_vec);
						}
					}
					file_action = render_playlist_elements(ui, &self.playlist_tree, &self.searched_playlist_tree, &self.playlists, song_depth, &audio_data.song_name);
				} else {
					ui.label("No saved playlists found");
				}
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
			FileActions::CloseDirectory(dir) => {
				self.directory_map.remove(&dir);
				self.active_search_text = "".to_string();
				self.directory_tree = None;
				self.searched_directory_tree = None;
			},
			FileActions::EnterDirectory(dir) => {
				self.active_directory_filepath = dir.clone();
				self.current_song_folder = dir;
				self.directory_tree = None;
				self.searched_directory_tree = None;
			},
			FileActions::OpenPlaylist(index) => {
				if let Some(playlist) = self.playlists.get_mut(index) {
					playlist.is_open = true;
					self.playlist_tree = None;
					self.active_search_text_playlists = "".to_string();
					ctx.request_repaint();
				}
			},
			FileActions::ClosePlaylist(index) => {
				if let Some(playlist) = self.playlists.get_mut(index) {
					playlist.is_open = false;
					self.playlist_tree = None;
					self.active_search_text_playlists = "".to_string();
					ctx.request_repaint();
				}
			},
			FileActions::EnterPlaylist(index) => {
				self.active_playlist_index = Some(index);
				self.playlist_tree = None;
				ctx.request_repaint();
			},
			FileActions::PlaySong(song) => {
				send_audio_signal(&self.audio_message_channel, MessageToAudio::PlaySong(song));
			}
		}
		
		if request_refresh {
			ctx.request_repaint();
			// active_directory = None;
			self.active_playlist_index = None;
			self.playlist_tree = None;
			self.searched_directory_tree = None;
			self.directory_tree = None;
			self.directory_map.clear();
		}

		egui::CentralPanel::default().show(ctx, |ui| {
			ui.vertical_centered(|ui| {
				ui.heading("Player Parameters");
				ui.add_space(5.0);
			});
			ui.horizontal(|ui| {
				ui.label("Audio Device: ");
				egui::ComboBox::from_label("")
					.selected_text("Default (unimplemented)")
					.show_ui(ui, |ui| {
						ui.selectable_value(&mut self.browse_mode, SongBrowseMode::Files, "Default (unimplemented)");
					}
				);
			});
			ui.horizontal(|ui| {
				ui.label("Speed: ");
				// Note to self: Fill color can be adjusted with ui.visuals_mut().selection.bg_fill = Color32::{INSERT COLOR HERE};
				ui.add(
					egui::Slider::new(&mut self.song_speed, 0.5..=2.0)
					.handle_shape(egui::style::HandleShape::Rect { aspect_ratio: 1.0 })
					.show_value(false)
					.trailing_fill(true)
					.logarithmic(true)
				);
				if ui.button("Reset").clicked() {
					self.song_speed = 1.0;
				}
			});
			ui.horizontal(|ui| {
				ui.label("Reverb: ");
				ui.add(
					egui::Slider::new(&mut self.song_reverb, 0.0..=1.0)
					.handle_shape(egui::style::HandleShape::Rect { aspect_ratio: 1.0 })
					.show_value(false)
					.trailing_fill(true)
					.logarithmic(true)
				);
				if ui.button("Reset").clicked() {
					self.song_reverb = 0.0;
				}
			});
			

			ui.vertical_centered(|ui| {
				ui.add_space(5.0);
				ui.heading("Song Info");
				ui.add_space(5.0);
			});
		});
	}
}

fn main() -> eframe::Result {
	let options = eframe::NativeOptions {
		viewport: egui::ViewportBuilder::default()
			.with_inner_size([800.0, 600.0]),
		..Default::default()
	};

	eframe::run_native(
		"Pinetree Music Player",
		options,
		Box::new(|_cc| Ok(Box::new(MyApp::default()))),
	)
}
