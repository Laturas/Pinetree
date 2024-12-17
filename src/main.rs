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
use egui::{Color32, RichText, TextWrapMode};
use std::{
	collections::HashMap,
	fs::{File, OpenOptions},
	io::{BufRead, BufReader, Write},
	path::Path,
	sync::{Arc, Mutex},
	time::{Duration, SystemTime},
};
use rodio::Source;
use rand::Rng;
use id3::{self, TagLike};

// This is a really stupid dependency but as it turns out I guess this is a non-trivial problem???
// Rodio's built in functionality for this just doesn't work most of the time for some reason.
use mp3_duration;

fn main() -> Result<(), eframe::Error> {
	let options = eframe::NativeOptions {
		viewport: egui::ViewportBuilder::default().with_inner_size([690.0, 360.0]),
		..Default::default()
	};
	let app = App::default();
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
		"Dreamer",
		options,
		Box::new(|_cc| {Ok(Box::new(app))}),
	)
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
enum SelectionType {None,Loop,Random,Next}

// This is everything that needs to be shared across threads
struct SharedAppData {
	sel_type: SelectionType,
	cur_song_index: usize,
	songs_list: Vec<String>,
	start_system: SystemTime,
	start_milis: u64,
	position: u64,
	total_duration: u64,
	current_song_info: SongInfo,
	dat_map: HashMap<String, String>,
	song_data_exists: bool,

	_stream: rodio::OutputStream, // THIS HAS TO EXIST otherwise the lifetime causes the program to crash
	sink: rodio::Sink,
}

impl Default for SharedAppData {
	fn default() -> Self {
		let mut songls: Vec<String> = vec![];
		let paths = std::fs::read_dir("songs\\");
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
			_stream: i1,
			sink: rodio::Sink::try_new(&i2).unwrap(),
			sel_type: SelectionType::None,
			cur_song_index: 0,
			songs_list: songls,
			start_system: SystemTime::now(),
			total_duration: 0,
			start_milis: 0,
			position: 0,
			current_song_info: new_si,
			dat_map: data_map,
			song_data_exists: data_found,
		}
	}
}

struct App {
	appdata: Arc<Mutex<SharedAppData>>,
	
	// Not accessed from other threads
	search_text: String,
	error: String,
	volume: f32,
	save_data_message: DataSaveError,
}

impl Default for App {
	fn default() -> Self {
		Self {
			appdata: Arc::new(Mutex::new(SharedAppData::default())),
			search_text: format!(""),
			error: format!(""),
			volume: 0.5,
			save_data_message: DataSaveError::NoError,
		}
	}
}

impl eframe::App for App {
	fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
		ctx.request_repaint_after(Duration::from_millis(250));
		ctx.set_visuals(Visuals::dark());
		ctx.set_pixels_per_point(1.33);

		egui::CentralPanel::default().show(ctx, |ui| {
			ui.heading("Kate's Untitled MP3 Player");
			ui.horizontal(|ui| {
				let mut appdata = self.appdata.lock().unwrap();
				ui.label("When a song ends: ");
				egui::ComboBox::from_label("")
					.selected_text(format!("{:?}", appdata.sel_type))
					.show_ui(ui, |ui| {
						ui.selectable_value(&mut appdata.sel_type, SelectionType::None, "None");
						ui.selectable_value(&mut appdata.sel_type, SelectionType::Loop, "Loop");
						ui.selectable_value(&mut appdata.sel_type, SelectionType::Random, "Random");
						ui.selectable_value(&mut appdata.sel_type, SelectionType::Next, "Next");
					}
				);
				//ui.checkbox(&mut self.loopy, "Loop songs on finish");
			});
			ui.horizontal(|ui| {
				if ui.button("Refresh").clicked() {
					let mut appdata = self.appdata.lock().unwrap();

					// This is incredibly weird but I had to do it this way to satisfy the borrow checker.
					let old_file_name = appdata.songs_list.get(appdata.cur_song_index);
					let (ofn, ofn_exists) = if let Some(old_file_name) = old_file_name {
						((*old_file_name).clone(), true)
					} else {
						(String::new(), false)
					};
					appdata.songs_list.clear();

					let paths = std::fs::read_dir("songs\\");
					if let Ok(paths) = paths {
						for p in paths {
							if let Ok(a) = p {
								let song_result = a.file_name().into_string();
								if let Ok(song_result) = song_result {
									if song_result.ends_with(".mp3") {
										appdata.songs_list.push(song_result);
									}
								}
							}
						}
					}
					// Rare windows W
					// Windows returns the files sorted alphabetically. But this is platform dependent behavior
					// To keep songs in order on non-windows platforms we run a sort() call.
					if !cfg!(windows) {
						appdata.songs_list.sort();
					}
					if ofn_exists {
						let new_index = appdata.songs_list.binary_search(&ofn);
						if let Ok(new_index) = new_index {
							appdata.cur_song_index = new_index;
						}
					} else {
						// This is only in the rare circumstance that the song playing has since been deleted before the refresh.
						appdata.cur_song_index = 0;
					}
				}
				ui.add(egui::TextEdit::singleline(&mut self.search_text).hint_text("Search..."));
			});
			ui.add_space(10.0);
			ui.horizontal(|ui| {
				ui.set_min_height(200.0);
				ui.vertical(|ui| {
					egui::ScrollArea::vertical().show(ui, |ui| {
						ui.set_max_width(275.0);
						ui.set_min_width(275.0);
						let mut song_change_triggered = false;
						let mut activate_song = 0;
						{
							let mut aplock = self.appdata.lock().unwrap();
							let current_song_index_clone = aplock.cur_song_index;
							for (index, dir) in (&mut aplock.songs_list).into_iter().enumerate() {
								if self.search_text.len() == 0 || 
									dir.to_ascii_lowercase().contains(&self.search_text.to_ascii_lowercase())
								{
									let mut clicked = false;
									ui.horizontal(|ui| {
										if current_song_index_clone == index {
											ui.set_max_width(245.0);
											ui.style_mut().wrap_mode = Some(TextWrapMode::Truncate);
											ui.label(RichText::new(dir.clone()).underline().strong());
										}
										else {
											ui.style_mut().wrap_mode = Some(TextWrapMode::Truncate);
											ui.set_max_width(245.0);
											ui.label(dir.clone());
										}
										if ui.button("▶").clicked() {
											song_change_triggered = true;
											clicked = true;
											activate_song = index;
										}
									});
								}
							}
						}
						if song_change_triggered {
							let res = {
								let mut appdata = self.appdata.lock().unwrap();

								// I will just assume this unwrap will never fail.
								// I cannot comprehend a scenario in which this would be triggered and also be OOB.
								let mut item = appdata.songs_list.get(activate_song).unwrap().clone();
								appdata.cur_song_index = activate_song;
								
								let data_exists  = update_cursong_data(&mut appdata, &mut item);
								let fp = format!("songs\\{}", item);
								let file = File::open(&fp).unwrap(); // HANDLE THIS at some point. This unwrap actually can fail.

								appdata.start_system = SystemTime::now();
								let reader = BufReader::new(file);
								appdata.song_data_exists = data_exists;
								(reader, fp)
							};
							
							self.save_data_message = DataSaveError::NoError;
							self.error = play_song(&mut self.appdata, res.0, &res.1);
						} else {
							let appdata = self.appdata.lock().unwrap();
							if appdata.songs_list.len() == 0 {
								ui.label("Error: No songs in active folder");
							}
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
						if ui.button("Save").clicked() {
							let sindex = appdata.cur_song_index;
							self.save_data_message = save_data(
								&mut appdata, sindex
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
						let item = appdata.songs_list.get(appdata.cur_song_index);
						let tag = if let Some(item) = item {let fp = format!("songs\\{}", item); Some(id3::Tag::read_from_path(&fp))} else {None};
						
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
				if let Some(song_name) = appdata.songs_list.get(appdata.cur_song_index as usize)  {
					if !appdata.sink.empty() {
						ui.label(format!("Currently Playing: {}", song_name));
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
						let tempsong = a_lock.songs_list.get(s_ind).clone();
						tempsong.is_some()
					};
					if song_exists {
						let fp = {
							let a_lock = self.appdata.lock().unwrap();

							// I similarly cannot comprehend a scenario where this unwrap fails. Cosmic bit flip or something maybe.
							format!("songs\\{}", a_lock.songs_list.get(a_lock.cur_song_index).unwrap()
						)};
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
							let sindex = appdatalock.cur_song_index;
							if appdatalock.songs_list.len() != 0 {
								save_data_noinsert(&mut appdatalock, sindex);
							}
							appdatalock.start_milis = appdatalock.position;
						},
					}
				}
				
				if ui.button("Stop").clicked() {
					let mut appdata = self.appdata.lock().unwrap();
					appdata.position = 0;
					appdata.start_system = SystemTime::now();
					appdata.start_milis = 0;
					appdata.sink.skip_one();
				}
				
				let og_spacing = ui.spacing().slider_width;
				let size = ctx.available_rect().size().x - 360.0;
				ui.spacing_mut().slider_width = size;

				
				let dragged = {
					let mut slappdata = self.appdata.lock().unwrap();

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
					appdata.position = appdata.start_system.elapsed().unwrap().as_millis() as u64 + appdata.start_milis;
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
			let to_save = {
				let sink = &appdata.lock().unwrap().sink;
				sink.stop();
				!sink.is_paused() && !sink.empty()
			};

			if to_save {
				let mut aplock = appdata.lock().unwrap();
				aplock.current_song_info.nodisplay_time_listened += aplock.start_system.elapsed().unwrap().as_millis();
				let sindex = aplock.cur_song_index;
				save_data_noinsert(
					&mut aplock, sindex
				);
			}
			// This lock cannot be merged into the one within the if statement because of the save data call.
			let mut appdata_mut = appdata.lock().unwrap();
			appdata_mut.start_system = SystemTime::now();
			appdata_mut.position = 0;
			appdata_mut.start_milis = 0;

			appdata_mut.sink.append(a.track_position()); 
			format!("")},
		Err(_) => format!("Error in decoding song :("),
	}
}

/// This writes the data out to the file, but if the song isn't already in the dataset it doesn't add it.
/// This makes it distinct from the saving function activated by the "save" button.
fn save_data_noinsert(app: &mut SharedAppData, cur_song_index: usize) {
	let current_song_info = &app.current_song_info;
	let dat_map = &mut app.dat_map;
	let songs_list = &app.songs_list;
	if let Some(current_s) = songs_list.get(cur_song_index) {
		let data = format!("{},{},{},{},{}", current_s, current_song_info.name, current_song_info.artist, current_song_info.genre, current_song_info.nodisplay_time_listened);
		
		if dat_map.contains_key(current_s) {
			dat_map.insert(current_s.clone(), data);
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

fn save_data(app: &mut SharedAppData, cur_song_index: usize) -> DataSaveError {
	if app.songs_list.len() == 0 {
		return DataSaveError::NoSongToSave;
	}
	let current_song_info = &app.current_song_info;
	let dat_map = &mut app.dat_map;
	let songs_list = &app.songs_list;
	if let Some(current_s) = songs_list.get(cur_song_index) {
		if current_song_info.name.contains(',') ||
			current_song_info.artist.contains(',') ||
			current_song_info.genre.contains(',')
		{
			return DataSaveError::IllegalChar;
		}
		let data = format!("{},{},{},{},{}", current_s, current_song_info.name, current_song_info.artist, current_song_info.genre, current_song_info.nodisplay_time_listened);
		
		dat_map.insert(current_s.clone(), data);
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

fn update_cursong_data(appdata: &mut SharedAppData, song_name: &str) -> bool {
	let map_data = appdata.dat_map.get(song_name);

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
	return match sel_type {
			SelectionType::None => {format!("")},
			SelectionType::Loop => {
			let fp = {
				let appdata = app.lock().unwrap();
				if let Some(s) = appdata.songs_list.get(appdata.cur_song_index) {
					format!("songs\\{}", s)
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
					let sindex = appdata.cur_song_index;
					save_data_noinsert(
						&mut appdata, sindex
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
				let sindex = appdata.cur_song_index;
				save_data_noinsert(&mut appdata, sindex);
				
				appdata.cur_song_index = if appdata.cur_song_index + 1 >= appdata.songs_list.len() {0} else {appdata.cur_song_index + 1};
				
				let mut item = appdata.songs_list.get(appdata.cur_song_index).unwrap().clone();
				let fp = format!("songs\\{}", item);
				appdata.song_data_exists = update_cursong_data(&mut appdata, &mut item);
				fp
			};
			let file = File::open(&fp);
			if let Ok(file) = file {
				let reader = BufReader::new(file);
				play_song(app, reader, &fp)
			} else {
				format!("Song you tried to play doesn't exist")
			}
		},
		SelectionType::Random => {
			let fp = {
				let mut appdata = app.lock().unwrap();
				appdata.current_song_info.nodisplay_time_listened += appdata.start_system.elapsed().unwrap().as_millis();
				appdata.start_system = SystemTime::now();
				let sindex = appdata.cur_song_index;
				save_data_noinsert(&mut appdata, sindex);
				
				appdata.cur_song_index = rand::thread_rng().gen_range(0..appdata.songs_list.len());
				
				// I won't even bother handling this bruh like come on.
				let mut item = appdata.songs_list.get(appdata.cur_song_index).unwrap().clone();
				let fp = format!("songs\\{}", item);
				appdata.song_data_exists = update_cursong_data(&mut appdata, &mut item);
				fp
			};
			let file = File::open(&fp);
			if let Ok(file) = file {
				let reader = BufReader::new(file);
				play_song(app, reader, &fp)
			} else {
				format!("Song you tried to play doesn't exist")
			}
		},
	};
}