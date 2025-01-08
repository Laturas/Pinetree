#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

/** 
*  REGARDING THE HIGH VOLUME OF unwrap() CALLS:
*  I have gone through and vetted all of them. Most of the remaining ones fall under one of these categories:
*  1. unwraps on a lock call - The only way a lock call can return an error is if the other thread panicked.
*     But I've guaranteed that can't happen, barring the second scenario:
*  2. unwraps on a system time call. If this fails I do not trust the state of the environment this program is running under,
*     and I cannot guarantee that it wont break something in one of the libraries I'm using.
*     It's best to just let the program crash at that point.
*
*  There's 2 file opening unwrap() calls I need to handle still, but that's it.
*  This program is as far as I know, otherwise incapable of crashing.
**/

use eframe::egui::Visuals;
use filetree::*;
use rodio::Source;
use rand::Rng;
use id3::TagLike;
use egui::{
	Color32,
	RichText,
	TextWrapMode, Vec2,
};
use std::{
	collections::HashMap,
	fs::{File, OpenOptions},
	io::{BufRead, BufReader, Write},
	path::Path,
	sync::{Arc, Mutex},
	time::{Duration, SystemTime},
};

// This is a really stupid dependency but as it turns out I guess this is a non-trivial problem???
// Rodio's built in functionality for this just doesn't work most of the time for some reason.
use mp3_duration;

mod filetree;

fn main() -> Result<(), eframe::Error> {
	let options = eframe::NativeOptions {
		viewport: egui::ViewportBuilder::default().with_inner_size([690.0, 360.0]).with_icon(load_icon()).with_min_inner_size(Vec2::new(670.0,360.0)),
		..Default::default()
	};
	let app = App::default();
	{
		let root_name_clone = {app.appdata.lock().unwrap().song_folder.clone()};

		let mut appdata = app.appdata.lock().unwrap();

		walk_tree(&mut appdata.prewalked, &root_name_clone, &app.filetree_hashmap);
	}
	let mut shared_data = app.appdata.clone();
	std::thread::spawn(move || {
        loop {
			std::thread::sleep(Duration::from_secs(1));
			let (seltype, activate) = {
				let datlock = shared_data.lock().unwrap();
				let sel_type = datlock.sel_type.clone();
				(sel_type, datlock.sink.empty())
			};
            if activate {
				let _ = handle_song_end(seltype, &mut shared_data);
			}
        }
	});
	eframe::run_native(
		"Pinetree Mp3 Player",
		options,
		Box::new(|_cc| {Ok(Box::new(app))}),
	)
}
use image;

pub(crate) fn load_icon() -> egui::IconData {
	let (icon_rgba, icon_width, icon_height) = {
		let icon = include_bytes!("../resources/Pinetree Logo.ico");
		let image = image::load_from_memory(icon)
			.expect("Failed to open icon path")
			.into_rgba8();
		let (width, height) = image.dimensions();
		let rgba = image.into_raw();
		(rgba, width, height)
	};
	
	egui::IconData {
		rgba: icon_rgba,
		width: icon_width,
		height: icon_height,
	}
}

struct SongInfo {
	name: String,
	artist: String,
	genre: String,
	nodisplay_time_listened: u128,
}

unsafe impl Send for SharedAppData {}

impl Default for SongInfo {
	fn default() -> Self {
		Self {
			name: format!(""),
			artist: format!(""),
			genre: format!(""),
			nodisplay_time_listened: 0,
		}
	}
}

/// Success = ran and completed without error
/// NoError = Has not run yet
enum DataSaveError {Success,NoError,FileOpenFail,NoSongToSave,NonexistentSong,IllegalChar}

#[derive(PartialEq)]
#[derive(Debug)]
#[derive(Clone)]
enum SelectionType {None,Loop,Shuffle,Next}

// This is everything that needs to be shared across threads
struct SharedAppData {
	search_text_results: String,
	search_results: Vec<usize>,
	sel_type: SelectionType,
	cur_song_index: usize,
	//songs_list: Vec<String>,
	start_system: SystemTime,
	song_folder: String,
	start_milis: u64,
	position: u64,
	total_duration: u64,
	current_song_info: SongInfo,
	dat_map: HashMap<String, String>,
	song_data_exists: bool,
	current_playing_song: Option<String>,

	_stream: rodio::OutputStream, // THIS HAS TO EXIST otherwise the lifetime causes the program to crash
	sink: rodio::Sink,

	prewalked: Vec<FileElement>,
}
struct App {
	appdata: Arc<Mutex<SharedAppData>>,
	
	// Not accessed from other threads
	search_text: String,
	genre_filter: String,
	artist_filter: String,
	searched_genre: String,
	searched_artist: String,
	error: String,
	volume: f32,
	save_data_message: DataSaveError,
	fonts_added: bool,
	force_refresh: bool,

	// There is a good reason I split this into two duplicate fields.
	//
	//	1. Updating the song list is a very expensive operation that I don't wanna do on every keystroke (would become VERY laggy, unavoidably).
	//	2. Updating every keystroke anyways would be pointless because on most of the keystrokes no result would be returned until the typing was finished
	//	3. I don't want song playing to break while the field is being typed in.
	//
	// So, this field stores what is shown in the text field,
	// and the one in appdata is what's used for any I/O operations and is updated on a refresh call.
	displayonly_song_folder: String,

	// This was originally gonna be a tree but I needed a hashmap anyways and it ended up working out
	filetree_hashmap: HashMap<String, FileTreeNode>,
}

impl Default for SharedAppData {
	fn default() -> Self {
		let mut songls: Vec<String> = vec![];
		let paths = std::fs::read_dir("songs/");
		let mut data_map: HashMap<String,String> = HashMap::new();

		initialize_data_map(&mut data_map);

		match paths {
			Ok(pat) => {
				for p in pat {
					if let Ok(a) = p {
						if let Ok(sn) = a.file_name().into_string() {
							if sn.ends_with(".mp3") {songls.push(sn);}
						}
					}
				}
			},
			Err(_) => {},
		}
		let data_found;
		let mut new_si = SongInfo::default();

		let item = songls.get(0);
		if let Some(item) = item {
			let map_data = data_map.get(item);
	
			data_found = if let Some(map_data) = map_data {
				let collection = map_data.split(',').collect::<Vec<&str>>();
	
				new_si.name = (**collection.get(1).unwrap_or(&"")).to_string();
				new_si.artist = (**collection.get(2).unwrap_or(&"")).to_string();
				new_si.genre = (**collection.get(3).unwrap_or(&"")).to_string();
				new_si.nodisplay_time_listened = (**collection.get(4).unwrap_or(&"")).to_string().parse().unwrap_or(0);
				true
			}
			else {
				false
			};
		} else {
			data_found = false;
		}
		let (i1, i2) = rodio::OutputStream::try_default().unwrap();
		
		
		Self {
			search_text_results: String::new(),
			search_results: Vec::new(),
			_stream: i1,
			sink: rodio::Sink::try_new(&i2).unwrap(),
			sel_type: SelectionType::None,
			cur_song_index: 0,
			//songs_list: songls,
			start_system: SystemTime::now(),
			song_folder: format!("songs/"),
			total_duration: 0,
			start_milis: 0,
			position: 0,
			current_song_info: new_si,
			dat_map: data_map,
			song_data_exists: data_found,

			prewalked: Vec::new(),
    		current_playing_song: None,
		}
	}
}


impl Default for App {
	fn default() -> Self {
		let default_directory = "songs/";
		let ad = Arc::new(Mutex::new(SharedAppData::default()));
		let rootnode = FileTreeNode::new(default_directory.to_owned());
		let mut ftree_hmap: HashMap<String, FileTreeNode> = HashMap::new();
		ftree_hmap.insert(default_directory.to_owned(), rootnode);
		Self {
			appdata: ad,
			search_text: format!(""),
			genre_filter: format!(""),
			artist_filter: format!(""),
			error: format!(""),
			volume: 0.5,
			save_data_message: DataSaveError::NoError,
			fonts_added: false,
			force_refresh: false,

			displayonly_song_folder: format!("songs/"),
			filetree_hashmap: ftree_hmap,
			searched_genre: format!(""),
			searched_artist: format!(""),
		}
	}
}

impl eframe::App for App {
	fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
		if !self.fonts_added {
			add_font(ctx);
		}
		ctx.request_repaint_after(Duration::from_millis(250));
		ctx.set_visuals(Visuals::dark());
		ctx.set_pixels_per_point(1.33);

		egui::CentralPanel::default().show(ctx, |ui| {
			ui.heading("Pinetree Mp3 Player");
			ui.horizontal(|ui| {
				let mut appdata = self.appdata.lock().unwrap();
				ui.label("When a song ends: ");
				egui::ComboBox::from_label("")
					.selected_text(format!("{:?}", appdata.sel_type))
					.show_ui(ui, |ui| {
						ui.selectable_value(&mut appdata.sel_type, SelectionType::None, "None");
						ui.selectable_value(&mut appdata.sel_type, SelectionType::Loop, "Loop");
						ui.selectable_value(&mut appdata.sel_type, SelectionType::Shuffle, "Shuffle");
						ui.selectable_value(&mut appdata.sel_type, SelectionType::Next, "Next");
					}
				);
				ui.label("File path:").on_hover_text("The file path of the current open folder.\nRelative to the file path of the executable"); 
				let lab = ui.add(egui::TextEdit::singleline(&mut self.displayonly_song_folder).hint_text("Song folder...")).on_hover_text("The file path of the current open folder.\nRelative to the file path of the executable");

				if lab.lost_focus() && lab.ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
					self.force_refresh = true;
				}
				if ui.button("←").on_hover_text_at_pointer("Backs up to the folder above").clicked() {
					self.displayonly_song_folder = {
						let last_char = {
							// This is here because otherwise with an extra slash it will back up to the same folder and just delete the slash.
							let operated_str = if self.displayonly_song_folder.ends_with('/') || self.displayonly_song_folder.ends_with('\\') {
								&self.displayonly_song_folder[..self.displayonly_song_folder.len() - 1]
							} else {
								&self.displayonly_song_folder
							};
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
						if last_char == 0 {format!("")}
						else {
							self.displayonly_song_folder[0..last_char].to_string()
						}
					};
					self.force_refresh = true;
				}
			});
			ui.horizontal(|ui| {
				if self.force_refresh {
					refresh_logic(self);
					self.force_refresh = false;
				}
				if ui.button("Refresh").on_hover_text("Hard refreshes the songs and directories list, clearing cached data in the process. Otherwise changes in file structure may not be reflected immediately.").clicked() {
					hard_refresh(self);
					self.force_refresh = false;
				}

				ui.add(egui::TextEdit::singleline(&mut self.search_text).hint_text("Search...").desired_width(175.0)).on_hover_text("Search for a given file name");
				
				ui.label("Filters:");
				ui.add(egui::TextEdit::singleline(&mut self.genre_filter).hint_text("Genre...").desired_width(130.0)).on_hover_text("Filter songs to those of a specific genre");
				ui.add(egui::TextEdit::singleline(&mut self.artist_filter).hint_text("Artist...").desired_width(130.0)).on_hover_text("Filter songs to those by a specific artist");
			});
			ui.add_space(10.0);
			ui.horizontal(|ui| {
				ui.set_min_height(200.0);
				ui.vertical(|ui| {
					let nonempty_search_terms = {
						   self.search_text.len() != 0
						|| self.artist_filter.len() != 0
						|| self.genre_filter.len() != 0
					};
					let mut use_search_results = {
						let aplock = self.appdata.lock().unwrap();
						nonempty_search_terms

						&& self.search_text == aplock.search_text_results
						&& self.artist_filter == self.searched_artist
						&& self.genre_filter == self.searched_genre
					};
					let dont_search = !nonempty_search_terms;

					let total = 
					if use_search_results {
						self.appdata.lock().unwrap().search_results.len()
					} else {
						if dont_search {
							let aplock = self.appdata.lock().unwrap();
							aplock.prewalked.len()
						} else {
							let mut aplock = self.appdata.lock().unwrap();
							let mut new_search_results: Vec<usize> = Vec::new();
							aplock.search_results.clear();
							for (index, dir) in (&aplock.prewalked).into_iter().enumerate() {
								let reduced = &reduce_song_name(&dir.file_name);
								if self.genre_filter.len() != 0 {
									if let Some(s) = aplock.dat_map.get(reduced) {
										if !s.split(',').into_iter().nth(3).unwrap_or("").to_lowercase().contains(&self.genre_filter.to_lowercase()) {
											continue;
										}
									} else {
										continue;
									}
								}
								if self.artist_filter.len() != 0 {
									if let Some(s) = aplock.dat_map.get(reduced) {
										if !s.split(',').into_iter().nth(2).unwrap_or("").to_lowercase().contains(&self.artist_filter.to_lowercase()) {
											continue;
										}
									} else {
										continue;
									}
								}
								//((&dir.file_name).to_lowercase()).contains(&self.search_text.to_lowercase())
								if self.search_text.len() == 0 || ((&dir.file_name).to_lowercase()).contains(&self.search_text.to_lowercase()) {
									new_search_results.push(index);
								}
							}
							use_search_results = true;
							aplock.search_text_results = self.search_text.clone();
							self.searched_artist = self.artist_filter.clone();
							self.searched_genre = self.genre_filter.clone();
							aplock.search_results = new_search_results;
							aplock.search_results.len()
						}
					};
					egui::ScrollArea::vertical().show_rows(ui, 16.0, total,|ui, row_range| {
						ui.set_max_width(275.0);
						ui.set_min_width(275.0);
						let mut song_change_triggered = false;
						let mut directory_activation = DirActivate::Inactive;
						let mut dirmove_name = String::new();
						let mut activate_song = 0;
						let prev_song = {
							let aplock = self.appdata.lock().unwrap();
							let current_song_clone = aplock.current_playing_song.clone();

							for row in row_range {
								let (element_index, file) = {
									if use_search_results {
										let pw_index = aplock.search_results.get(row).unwrap();
										let file = aplock.prewalked.get(*pw_index).unwrap();
										(*pw_index, file)
									} else {
										let file = aplock.prewalked.get(row).unwrap();
										(row, file)
									}
								};
								match file.file_type {
									FileType::Directory => {
										(directory_activation, dirmove_name) = {
											let tmp = render_directory_element(ui, self.filetree_hashmap.contains_key(&file.file_name), &file.file_name);
											if tmp == DirActivate::Inactive {(directory_activation, dirmove_name)} else {(tmp, file.file_name.clone())}
										};
									},
									FileType::AudioFile => {
										let cursong = &aplock.current_playing_song;
										if render_song_entry_ui_element(ui, element_index, cursong, &file.file_name, &mut activate_song) {
											song_change_triggered = true;
											activate_song = element_index;
										}
									},
								}
							}
							current_song_clone
						};
						// BUG: Currently the program assumes the previous song is in the same folder which unfortunate transfers song listen duration incorrectly.
						if song_change_triggered {
							let res = {
								// I will just assume this unwrap will never fail.
								// I cannot comprehend a scenario in which this would be triggered and also be OOB.
								let item = self.appdata.lock().unwrap().prewalked.get(activate_song).unwrap().file_name.clone();
								if let Some(old_song) = prev_song {
									let mut appdata = self.appdata.lock().unwrap();

									if !appdata.sink.is_paused() && !appdata.sink.empty() {
										appdata.current_song_info.nodisplay_time_listened += appdata.start_system.elapsed().unwrap().as_millis();
									}
									
									save_data_noinsert(&mut appdata, Some(old_song));
									appdata.cur_song_index = activate_song;
								}
								
								let (data_exists, fp)  = {
									let mut appdata = self.appdata.lock().unwrap();
									(update_cursong_data(&mut appdata, &item), item)
								};
								let file = File::open(&fp);
								
								let mut appdata = self.appdata.lock().unwrap();

								
								appdata.start_system = SystemTime::now();
								let reader: Option<BufReader<File>> = if let Ok(file) = file {
									Some(BufReader::new(file))
								} else {None};
								appdata.song_data_exists = data_exists;
								(reader, fp)
							};
							
							self.save_data_message = DataSaveError::NoError;
							self.error = if let Some(file_reader) = res.0 {
								play_song(&mut self.appdata, file_reader, &res.1)
							} else {
								format!("Failed to read file (was this song deleted?)")
							};
						} else {
							let appdata = self.appdata.lock().unwrap();
							if appdata.prewalked.len() == 0 {
								ui.label("Error: No songs in active folder");
							}
						}

						match directory_activation {
							DirActivate::Inactive => (),
							DirActivate::Remove => {
								self.filetree_hashmap.remove(&dirmove_name);
	
								self.force_refresh = true;
							},
							DirActivate::Add => {
								self.filetree_hashmap.insert(dirmove_name.clone(), FileTreeNode::new(dirmove_name));

								self.force_refresh = true;
							},
							DirActivate::Enter => {
								self.displayonly_song_folder = dirmove_name;
								self.force_refresh = true;
							},
						}
					});
				});
				
				ui.vertical(|ui| {
					ui.set_max_width(250.0);
					ui.set_min_width(250.0);
					let mut appdata = self.appdata.lock().unwrap();
					ui.vertical_centered(|ui| {
						ui.heading("Song Info");
					});
					ui.horizontal(|ui| {
						let song_label = ui.label("Song");
						ui.text_edit_singleline(&mut appdata.current_song_info.name).labelled_by(song_label.id);
					});
					ui.horizontal(|ui| {
						let artist_label = ui.label("Artist");
						ui.text_edit_singleline(&mut appdata.current_song_info.artist).labelled_by(artist_label.id);
					});
					ui.horizontal(|ui| {
						let genre_label = ui.label("Genre");
						ui.text_edit_singleline(&mut appdata.current_song_info.genre).labelled_by(genre_label.id);
					});
					ui.horizontal(|ui| {
						if ui.button("Save").on_hover_text("Saves the data to a file").clicked() {
							let current_song = appdata.current_playing_song.clone();
							self.save_data_message = save_data(
								&mut appdata, current_song
							);
							appdata.song_data_exists = true;
						}
						if !appdata.song_data_exists {
							ui.horizontal( |ui| {
								ui.label(RichText::new("Warning:").color(Color32::YELLOW));
								ui.label("No saved data found");
							});
						}
						match self.save_data_message {
							DataSaveError::NoError => (),
							DataSaveError::Success => {ui.label("Data saved successfully").on_hover_cursor(egui::CursorIcon::Default);},
	
							DataSaveError::FileOpenFail => {ui.label("Error: Couldn't open file").on_hover_text("Couldn't open the file to save the data (is data.csv open in another program?)").on_hover_cursor(egui::CursorIcon::Default);}
							DataSaveError::NoSongToSave => {ui.label("Error: No active song").on_hover_text("There is no active song to save the data for").on_hover_cursor(egui::CursorIcon::Default);}
							DataSaveError::NonexistentSong => {ui.label("Error: Song doesn't exist").on_hover_text("The song you tried to save does not exist").on_hover_cursor(egui::CursorIcon::Default);}
							DataSaveError::IllegalChar => {ui.label("Fields can't have commas (,)").on_hover_text("Data is stored in the csv format, and storing commas would break the parsing.").on_hover_cursor(egui::CursorIcon::Default);}
						};
					});
					ui.separator();
					ui.vertical_centered_justified(|ui| {
						ui.heading("Metadata");
					});
					
					egui::Frame {
						inner_margin: egui::Margin{left: 2., right: 0.,top: 3.,bottom: 3.,},
						outer_margin: egui::Margin{left: 2., right: 2.,top: 2.,bottom: 20.,},
						stroke: egui::Stroke::new(1.0,Color32::DARK_GRAY),
						..Default::default()
					}.fill(Color32::BLACK).show(ui, |ui| {
						ui.set_max_width(250.0);
						ui.set_min_width(250.0);
						//ui.style_mut().wrap_mode = Some(TextWrapMode::Truncate);
						let item = appdata.current_playing_song.clone();
						let tag = if let Some(item) = item {
							Some(id3::Tag::read_from_path(&item))
						} else {None};
						
						ui.set_min_height(25.0);
						ui.set_max_height(25.0);
						if let Some(tag) = tag {
							if let Ok(tag) = tag {
								egui::ScrollArea::vertical().show(ui, |ui| {
									ui.set_max_width(250.0);
									ui.set_min_width(250.0);
									if let Some(artist) = tag.artist() {
										ui.label(egui::RichText::new(format!("Artist: {}", artist)).background_color(Color32::BLACK).size(13.0).line_height(Some(16.0)));
									}
									if let Some(title) = tag.title() {
										ui.label(egui::RichText::new(format!("Title: {}", title)).background_color(Color32::BLACK).size(13.0).line_height(Some(16.0)));
									}
									if let Some(album) = tag.album() {
										ui.label(egui::RichText::new(format!("Album: {}", album)).background_color(Color32::BLACK).size(13.0).line_height(Some(16.0)));
									}
								});
							} else {
								ui.label(egui::RichText::new(format!("No metadata found")).background_color(Color32::BLACK).size(14.0).line_height(Some(16.0)));
							}
						}
					});
				});
			});
		});

		egui::TopBottomPanel::bottom("Player").show(ctx, |ui| {
			ui.horizontal(|ui| {
				let appdata = self.appdata.lock().unwrap();
				
				// If you know of a way of combining these let me know, cuz I don't know a better way.
				if let Some(song) = &appdata.current_playing_song  {
					if !appdata.sink.empty() {
						ui.label(format!("Currently Playing: {}", reduce_song_name(song)));
					} else {
						ui.label(format!("No song currently playing"));
					}
				} else {
					ui.label(format!("No song currently playing"));
				}
				
				ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
					ui.label(&self.error)
				});
				
			});
			ui.horizontal(|ui| {
				if ui.button("Play").clicked() {
					let song_exists= {
						let a_lock = self.appdata.lock().unwrap();
						let s_ind = a_lock.cur_song_index;
						let tempsong = a_lock.prewalked.get(s_ind).clone();
						tempsong.is_some()
					};
					if song_exists {
						let fp = {
							let a_lock = self.appdata.lock().unwrap();

							// I similarly cannot comprehend a scenario where this unwrap fails. Cosmic bit flip or something maybe.
							a_lock.prewalked.get(a_lock.cur_song_index).unwrap().file_name.clone()
						};
						let open_file = File::open(&fp);
	
						if let Ok(open_file) = open_file {
							let reader = BufReader::new(open_file);
							
							self.save_data_message = DataSaveError::NoError;
							self.error = play_song(&mut self.appdata, reader, &fp);
						}
						else {
							self.error = format!("File not found: {}", &fp);
						}
					}
				}
				// Scope here to prevent a deadlock
				{
					let mut appdatalock = self.appdata.lock().unwrap();
					match appdatalock.sink.is_paused() {
						true => if ui.button("Unpause").clicked() {
							appdatalock.sink.play();
							appdatalock.start_system = SystemTime::now()
						},
						false => if ui.button("Pause").clicked() {
							appdatalock.sink.pause();
							appdatalock.current_song_info.nodisplay_time_listened += appdatalock.start_system.elapsed().unwrap().as_millis();
							let currentsong = appdatalock.current_playing_song.clone();
							if appdatalock.prewalked.len() != 0 {
								save_data_noinsert(&mut appdatalock, currentsong);
							}
							appdatalock.start_milis = appdatalock.position;
						},
					}
				}
				
				if ui.button("Skip").clicked() {
					let sel_type = {
						let mut appdata = self.appdata.lock().unwrap();
						appdata.position = 0;
						appdata.sink.stop();
						appdata.sel_type.clone()
					};
					self.error = handle_song_end(sel_type, &mut self.appdata);
					
					let mut appdata = self.appdata.lock().unwrap();
					appdata.start_system = SystemTime::now();
					appdata.start_milis = 0;
				}
				
				let og_spacing = ui.spacing().slider_width;
				let size = ctx.available_rect().size().x - 360.0;
				ui.spacing_mut().slider_width = size;

				
				let dragged = {
					let mut slappdata = self.appdata.lock().unwrap();

					slappdata.position = slappdata.sink.get_pos().as_millis() as u64;
					let secs = slappdata.sink.get_pos().as_millis() / 1000;
					let max_duration = slappdata.total_duration;
					
					let seeker = ui.add(
						egui::Slider::new(&mut slappdata.position, 0..=max_duration)
						.handle_shape(egui::style::HandleShape::Rect { aspect_ratio: 1.0 })
						.show_value(false)
						.text(format!("{}:{}{}", secs / 60, if secs % 60 < 10 {"0"} else {""}, secs % 60))
						.trailing_fill(true)
						// Fill color can be adjusted with ui.visuals_mut().selection.bg_fill = Color32::{INSERT COLOR HERE};
					);
					ui.spacing_mut().slider_width = og_spacing;
					seeker.dragged()
				};

				// This is to prevent an issue that would cause an infinite loop somehow
				if dragged {
					let mut appdata = self.appdata.lock().unwrap();
					let _ = appdata.sink.try_seek(Duration::from_millis(appdata.position));
					appdata.current_song_info.nodisplay_time_listened += appdata.start_system.elapsed().unwrap().as_millis();
					appdata.start_system = SystemTime::now();
					appdata.start_milis = appdata.position;
				} else {
					let (sel_type, empt) = {
						let appdata = self.appdata.lock().unwrap();
						let empt = appdata.sink.empty();
						(appdata.sel_type.clone(), empt)
					};
					if empt {
						self.error = handle_song_end(sel_type, &mut self.appdata);
					}
				}
				let mut appdata = self.appdata.lock().unwrap();
				let pos = appdata.position;
				let tot_dur = appdata.total_duration;
				let add_values = {!appdata.sink.is_paused() && !appdata.sink.empty()};
				if pos < tot_dur && add_values {
					appdata.position = appdata.sink.get_pos().as_millis() as u64;
				}
				
				ui.with_layout(egui::Layout::right_to_left(egui::Align::RIGHT), |ui| {
					ui.add( egui::Slider::new(&mut self.volume, -0.2..=1.0)
						.show_value(false)
						.text("Volume")
						.trailing_fill(true)
					);

					let falloff = volume_curve(self.volume);
					if appdata.sink.volume() != falloff {
						appdata.sink.set_volume(falloff);
					}
				});
			});
		});
	}
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

fn play_song(appdata: &mut Arc<Mutex<SharedAppData>>, reader: BufReader<File>, fp: &str) -> String {
	let elem = rodio::Decoder::new_mp3(reader);
	match elem {
		Ok(a) => {
			let path = Path::new(&fp);
			let path_test = mp3_duration::from_path(&path);

			// Scope for anti-deadlock measures.
			{
				let mut mut_appdata = appdata.lock().unwrap();
				if let Ok(path_test) = path_test {
					mut_appdata.total_duration = path_test.as_millis() as u64;
				} else {
					let t = a.total_duration();
					if let Some(t) = t {
						mut_appdata.total_duration = t.as_millis() as u64;
					} else {
						return format!("Error - Couldn't determine song length");
					}
				}
				mut_appdata.total_duration = mp3_duration::from_path(&path).unwrap().as_millis() as u64;
			}
			let _ = {
				let sink = &appdata.lock().unwrap().sink;
				sink.stop();
				!sink.is_paused() && !sink.empty()
			};

			//if to_save {
			//	let mut aplock = appdata.lock().unwrap();
			//	aplock.current_song_info.nodisplay_time_listened += aplock.start_system.elapsed().unwrap().as_millis();
			//	let current_song = aplock.current_playing_song.clone();
			//	save_data_noinsert(
			//		&mut aplock, current_song
			//	);
			//}
			// This lock cannot be merged into the one within the if statement because of the save data call.
			let mut appdata_mut = appdata.lock().unwrap();
			appdata_mut.start_system = SystemTime::now();
			appdata_mut.position = 0;
			appdata_mut.start_milis = 0;

			appdata_mut.sink.append(a.track_position()); 
			appdata_mut.current_playing_song = Some(fp.to_owned());
			appdata_mut.cur_song_index = if let Some(num) = appdata_mut.prewalked.iter().enumerate().find(|s| s.1.file_name == *appdata_mut.current_playing_song.as_ref().unwrap()) {num.0} else {0};
			format!("")},
		Err(_) => format!("Error in decoding song :("),
	}
}

/// This writes the data out to the file, but if the song isn't already in the dataset it doesn't add it.
/// This makes it distinct from the saving function activated by the "save" button.
fn save_data_noinsert(app: &mut SharedAppData, current_song: Option<String>) {
	let current_song_info = &app.current_song_info;
	let dat_map = &mut app.dat_map;
	if let Some(current_s) = current_song {
		let sname = reduce_song_name(&current_s);
		let data = format!("{},{},{},{},{}", sname, current_song_info.name, current_song_info.artist, current_song_info.genre, current_song_info.nodisplay_time_listened);
		
		if dat_map.contains_key(&sname) {
			dat_map.insert(sname, data);
		} else {
			return;
		}
		let write_result = std::fs::write("data.csv", "");

		if let Err(_) = write_result {return;}

		let f = OpenOptions::new().append(true).open("data.csv");

		if let Ok(mut f) = f {
			for keys in dat_map.keys() {
				let _ = writeln!(f, "{}", dat_map.get(keys).unwrap()).is_ok();
			}
		}
	}
}

fn save_data(app: &mut SharedAppData, current_song: Option<String>) -> DataSaveError {
	if app.prewalked.len() == 0 {
		return DataSaveError::NoSongToSave;
	}
	let current_song_info = &app.current_song_info;
	let dat_map = &mut app.dat_map;
	if let Some(current_s) = current_song {
		if current_song_info.name.contains(',') ||
			current_song_info.artist.contains(',') ||
			current_song_info.genre.contains(',')
		{
			return DataSaveError::IllegalChar;
		}
		let sname = reduce_song_name(&current_s);
		let data = format!("{},{},{},{},{}", sname, current_song_info.name, current_song_info.artist, current_song_info.genre, current_song_info.nodisplay_time_listened);
		
		dat_map.insert(sname, data);
		let write_result = std::fs::write("data.csv", "");

		if let Err(_) = write_result {
			return DataSaveError::FileOpenFail;
		}

		let f = OpenOptions::new().append(true).open("data.csv");

		if let Ok(mut f) = f {
			for keys in dat_map.keys() {
				let _ = writeln!(f, "{}", dat_map.get(keys).unwrap()).is_ok();
			}
			return DataSaveError::Success;
		} else {
			return DataSaveError::FileOpenFail;
		}
	} else {
		return DataSaveError::NonexistentSong;
	}
}

fn initialize_data_map(data_map: &mut HashMap<String,String>) {
	let fp = format!("data.csv");
	let file = File::open(&fp);

	if let Ok(file) = file {
		let reader = BufReader::new(file);
		for line in reader.lines() {
			if let Ok(line) = line {
				let collection = line.split(',').collect::<Vec<&str>>();

				// Tried to find a scenario where this fails but came up short. Oh well.
				// It shouldn't cause a crash or any weird behavior regardless because it's properly handled.
				if let Some(got_key) = collection.get(0) {
					let key = (**got_key).to_string();
					data_map.insert(key, line);
				}
			}
		}
	}
}

//fn get_song_data(appdata: SharedAppData, song_name: &str) -> Option<SongInfo> {
//	let map_data = appdata.dat_map.get(&reduce_song_name(song_name));
//
//	if let Some(map_data) = map_data {
//		let new_song_info: SongInfo = SongInfo {
//			name: todo!(),
//			artist: todo!(),
//			genre: todo!(),
//			nodisplay_time_listened: todo!(),
//		};
//		let collection = map_data.split(',').collect::<Vec<&str>>();
//
//		appdata.current_song_info.name = (**collection.get(1).unwrap_or(&format!("").as_str())).to_string();
//		appdata.current_song_info.artist = (**collection.get(2).unwrap_or(&format!("").as_str())).to_string();
//		appdata.current_song_info.genre = (**collection.get(3).unwrap_or(&format!("").as_str())).to_string();
//		appdata.current_song_info.nodisplay_time_listened = (**collection.get(4).unwrap_or(&format!("").as_str())).to_string().parse().unwrap_or(0);
//		return true;
//	} else {
//		return None
//	}
//}

fn update_cursong_data(appdata: &mut SharedAppData, song_name: &str) -> bool {
	let map_data = appdata.dat_map.get(&reduce_song_name(song_name));

	if let Some(map_data) = map_data {
		let collection = map_data.split(',').collect::<Vec<&str>>();

		appdata.current_song_info.name = (**collection.get(1).unwrap_or(&format!("").as_str())).to_string();
		appdata.current_song_info.artist = (**collection.get(2).unwrap_or(&format!("").as_str())).to_string();
		appdata.current_song_info.genre = (**collection.get(3).unwrap_or(&format!("").as_str())).to_string();
		appdata.current_song_info.nodisplay_time_listened = (**collection.get(4).unwrap_or(&format!("").as_str())).to_string().parse().unwrap_or(0);
		return true;
	} else {
		appdata.current_song_info.name = format!("");
		appdata.current_song_info.nodisplay_time_listened = 0;
		return false;
	}
}

/// Returns error text to be displayed
fn handle_song_end(sel_type: SelectionType, app: &mut Arc<Mutex<SharedAppData>>) -> String {
	{
		// TODO
		if sel_type != SelectionType::None && app.lock().unwrap().prewalked.len() == 0 {
			return format!("Error: No songs in current directory");
		}
	}
	return match sel_type {
			SelectionType::None => {format!("")},
			SelectionType::Loop => {
			let fp = {
				let appdata = app.lock().unwrap();
				if let Some(s) = appdata.current_playing_song.clone() {
					s
				} else {
					return format!("How did you even cause this error??");
				}
			};
			let open_file = File::open(&fp);
			if let Ok(open_file) = open_file {
				let reader = {
					let mut appdata = app.lock().unwrap();
					let reader = BufReader::new(open_file);
					
					appdata.current_song_info.nodisplay_time_listened += appdata.start_system.elapsed().unwrap().as_millis();
					let current_song = appdata.current_playing_song.clone();
					save_data_noinsert(
						&mut appdata, current_song
					);
					reader
				};
				
				play_song(app, reader, &fp)
			}
			else {
				format!("File not found: {}", &fp)
			}
		},
		SelectionType::Next => {
			let fp = {
				let mut appdata = app.lock().unwrap();
				appdata.current_song_info.nodisplay_time_listened += appdata.start_system.elapsed().unwrap().as_millis();
				appdata.start_system = SystemTime::now();
				let current_song = appdata.current_playing_song.clone();
				save_data_noinsert(
					&mut appdata, current_song
				);
				let start = appdata.cur_song_index;
				let max = appdata.prewalked.len();
				let item: Option<String> = get_next_song_bounded(&mut appdata, start, max);
				
				if let Some(item) = item {
					appdata.song_data_exists = update_cursong_data(&mut appdata, &item);
					Some(item)
				} else {
					None
				}
			};
			if let Some(fp) = fp {
				let file = File::open(&fp);
				if let Ok(file) = file {
					let reader = BufReader::new(file);
					play_song(app, reader, &fp)
				} else {
					format!("Song you tried to play doesn't exist")
				}
			} else {
				format!("Song you tried to play doesn't exist")
			}
		},
		SelectionType::Shuffle => {
			let fp = {
				let mut appdata = app.lock().unwrap();
				appdata.current_song_info.nodisplay_time_listened += appdata.start_system.elapsed().unwrap().as_millis();
				appdata.start_system = SystemTime::now();
				let current_song = appdata.current_playing_song.clone();
				save_data_noinsert(
					&mut appdata, current_song
				);
				
				let item: Option<String> = get_random_song_bounded(&mut appdata);
				
				if let Some(item) = item {
					appdata.song_data_exists = update_cursong_data(&mut appdata, &item);
					Some(item)
				} else {
					None
				}
			};
			if let Some(fp) = fp {
				let file = File::open(&fp);
				if let Ok(file) = file {
					let reader = BufReader::new(file);
					play_song(app, reader, &fp)
				} else {
					format!("Song you tried to play doesn't exist")
				}
			} else {
				format!("Song you tried to play doesn't exist")
			}
		},
	};
}

#[derive(PartialEq)]
enum DirActivate {
	// Default - Nothing happens.
	Inactive,
	// Adds the songs of this folder to the current song list
	Add,
	// Removes the songs of this folder from the current song list
	Remove,
	// Enters this directory as the new current root directory
	Enter,
}

/// **dir_active**: Bool for whether or not this directory is already added
fn render_directory_element(ui: &mut egui::Ui, dir_active: bool, text: &str) -> DirActivate {
	let mut dir_activation = DirActivate::Inactive;
	ui.horizontal(|ui| {
			ui.style_mut().wrap_mode = Some(TextWrapMode::Truncate);
			ui.set_max_width(245.0);
			ui.scope(|ui| {
				ui.style_mut().visuals.hyperlink_color = Color32::from_rgb(180, 180, 255);
				if ui.add(egui::Link::new(reduce_song_name(text))).on_hover_text_at_pointer("Enter this folder").clicked() {
					dir_activation = DirActivate::Enter;
				}
			});
		if ui.button(RichText::new(if dir_active {"−"} else {"+"}).strong().size(16.0)).on_hover_text("Add songs from this folder to the current list").clicked() {
			dir_activation = if dir_active {DirActivate::Remove} else {DirActivate::Add};
		}
	});
	return dir_activation;
}

fn render_song_entry_ui_element(ui: &mut egui::Ui, index: usize, current_song: &Option<String>, dir: &str,
	activate_song: &mut usize) -> bool
{
	let mut return_value= false;
	let reduced_current: Option<String> = if let Some(current_song) = current_song {Some(reduce_song_name(&current_song))} else {None};
	let reduced_torender = reduce_song_name(dir);
	ui.horizontal(|ui| {
		if let Some(reduced_current) = reduced_current {
			if reduced_current == reduced_torender {
				ui.set_max_width(245.0);
				ui.style_mut().wrap_mode = Some(TextWrapMode::Truncate);
				ui.label(RichText::new(reduced_torender).underline().strong());
			}
			else {
				ui.style_mut().wrap_mode = Some(TextWrapMode::Truncate);
				ui.set_max_width(245.0);
				ui.label(reduced_torender);
			}
		} else {
			ui.style_mut().wrap_mode = Some(TextWrapMode::Truncate);
			ui.set_max_width(245.0);
			ui.label(reduced_torender);
		}
		if ui.button("▶").clicked() {
			
			*activate_song = index;
			return_value = true;
		}
	});
	return return_value;
}


// Demonstrates how to add a font to the existing ones
fn add_font(ctx: &egui::Context) {
	let mut fonts = egui::FontDefinitions::default();

	fonts.font_data.insert("fallback".to_owned(),
	egui::FontData::from_static(include_bytes!(
			"./../fonts/MPLUS1p-Regular.ttf"
		))
	);

	fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .push("fallback".to_owned());
    ctx.set_fonts(fonts);
}

fn refresh_logic(app: &mut App) {
	// Not resetting this could break things in a billion tiny edge cases and I am NOT handling that.
	app.search_text = format!("");
					
	let mut appdata = app.appdata.lock().unwrap();
	appdata.song_folder = if app.displayonly_song_folder.len() == 0 || app.displayonly_song_folder.ends_with('/') || app.displayonly_song_folder.ends_with('\\') {
		app.displayonly_song_folder.clone()
	} else {
		format!("{}/", app.displayonly_song_folder)
	};
	
	appdata.current_song_info.nodisplay_time_listened += appdata.start_system.elapsed().unwrap().as_millis();
	appdata.start_system = SystemTime::now();
	let current_song = appdata.current_playing_song.clone();
	save_data_noinsert(
		&mut appdata, current_song
	);
	
	// This is incredibly weird but I had to do it this way to satisfy the borrow checker.
	let old_file_name = appdata.prewalked.get(appdata.cur_song_index);
	let (ofn, ofn_exists) = if let Some(old_file_name) = old_file_name {
		((*old_file_name).file_name.clone(), true)
	} else {
		(String::new(), false)
	};
	let root_name = appdata.song_folder.clone();

	if !app.filetree_hashmap.contains_key(&root_name) {
		app.filetree_hashmap.insert(root_name.clone(), FileTreeNode::new(root_name.clone()));
	}

	walk_tree(&mut appdata.prewalked, &root_name, &app.filetree_hashmap);

	if ofn_exists {
		let new_pos = appdata.prewalked.iter().enumerate().find(|s| {s.1.file_type == FileType::AudioFile && s.1.file_name == ofn});
		if let Some(new_index) = new_pos {
			appdata.cur_song_index = new_index.0;
		}
	} else {
		// This is only in the rare circumstance that the song playing has since been deleted before the refresh.
		appdata.cur_song_index = 0;
	}
	let rslt = appdata.current_playing_song.clone();
	if let Some(song) = rslt {
		update_cursong_data(&mut appdata, &song);
	}
}

/// Completely resets the hashmap and cached data for a fresh look into the file tree.
/// Only triggers when you click "Refresh"
fn hard_refresh(app: &mut App) {
	// Not resetting this could break things in a billion tiny edge cases and I am NOT handling that.
	app.search_text = format!("");
					
	let mut appdata = app.appdata.lock().unwrap();
	appdata.song_folder = if app.displayonly_song_folder.len() == 0 || app.displayonly_song_folder.ends_with('/') || app.displayonly_song_folder.ends_with('\\') {
		app.displayonly_song_folder.clone()
	} else {
		format!("{}/", app.displayonly_song_folder)
	};
	
	appdata.current_song_info.nodisplay_time_listened += appdata.start_system.elapsed().unwrap().as_millis();
	appdata.start_system = SystemTime::now();
	let current_song = appdata.current_playing_song.clone();
	save_data_noinsert(
		&mut appdata, current_song
	);
	
	// This is incredibly weird but I had to do it this way to satisfy the borrow checker.
	let old_file_name = appdata.prewalked.get(appdata.cur_song_index);
	let (ofn, ofn_exists) = if let Some(old_file_name) = old_file_name {
		((*old_file_name).file_name.clone(), true)
	} else {
		(String::new(), false)
	};
	let root_name = appdata.song_folder.clone();

	let rootnode = FileTreeNode::new(root_name.to_owned());
	let mut ftree_hmap: HashMap<String, FileTreeNode> = HashMap::new();
	ftree_hmap.insert(root_name.to_owned(), rootnode);
	app.filetree_hashmap = ftree_hmap;

	walk_tree(&mut appdata.prewalked, &root_name, &app.filetree_hashmap);

	if ofn_exists {
		let new_pos = appdata.prewalked.iter().enumerate().find(|s| {s.1.file_type == FileType::AudioFile && s.1.file_name == ofn});
		if let Some(new_index) = new_pos {
			appdata.cur_song_index = new_index.0;
		}
	} else {
		// This is only in the rare circumstance that the song playing has since been deleted before the refresh.
		appdata.cur_song_index = 0;
	}
	let rslt = appdata.current_playing_song.clone();
	if let Some(song) = rslt {
		update_cursong_data(&mut appdata, &song);
	}
}

/// Removes all folders above the song, leaving only the file name
fn reduce_song_name(song_name: &str) -> String {
	let last_char = {
		let last_bslash = song_name.rfind('\\');
		let last_slash = song_name.rfind('/');

		match (last_bslash, last_slash) {
			(Some(bslash), Some(slash)) => {
				if bslash > slash {bslash} else {slash}
			}
			(Some(bslash), None) => bslash,
			(None, Some(slash)) => slash,
			(None, None) => 0,
		}
	};
	let slice = &song_name[last_char..];
	if slice.starts_with('/') || slice.starts_with('\\') {
		format!("{}", &song_name[last_char + 1..])
	} else {
		format!("{}", &song_name[last_char..])
	}
}

/// This function is built to fail if it can't find any songs to play after 100 iterations
fn get_random_song_bounded(appdata: &mut SharedAppData) -> Option<String> {
	for _ in 0..100 {
		let random_index = rand::thread_rng().gen_range(0..appdata.prewalked.len());
		let name = appdata.prewalked.get(random_index).unwrap().file_name.clone();
		let itemtype = &appdata.prewalked.get(random_index).unwrap().file_type;
		if *itemtype == FileType::AudioFile {
			return Some(name);
		}
	}
	None
}

fn get_next_song_bounded(appdata: &mut SharedAppData, start_index: usize, vec_size: usize) -> Option<String> {
	for e in start_index + 1..start_index + 128 {
		let random_index = e % vec_size;
		let name = appdata.prewalked.get(random_index).unwrap().file_name.clone();
		let itemtype = &appdata.prewalked.get(random_index).unwrap().file_type;
		if *itemtype == FileType::AudioFile {
			return Some(name);
		}
	}
	None
}