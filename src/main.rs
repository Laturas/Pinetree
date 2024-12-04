#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use eframe::{egui, egui::Visuals};
//use egui::ahash::HashMap;
use std::{collections::HashMap, fs::{self, File, OpenOptions}, io::{BufRead, BufReader, Write}, path::Path, str, time::{Duration, SystemTime}};
use rodio::{OutputStream, Source};

// This is a really stupid dependency but as it turns out I guess this is a non-trivial problem???
// Rodio's built in functionality for this just doesn't work most of the time for some reason.
use mp3_duration;

fn main() -> Result<(), eframe::Error> {
	let options = eframe::NativeOptions {
		viewport: egui::ViewportBuilder::default().with_inner_size([690.0, 340.0]),
		..Default::default()
	};
	eframe::run_native(
		"Dreamer",
		options,
		Box::new(|_cc| {Box::<App>::default()}),
	)
}

//static mut DATA: Vec<(String, String)> = Vec::new();

struct SongInfo {
	name: String,
	artist: String,
	genre: String,
	nodisplay_time_listened: u128,
}

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

#[derive(PartialEq)]
#[derive(Debug)]
enum SelectionType {All,Artist,Song}

struct App {
	sink: rodio::Sink,
	_stream: OutputStream, // THIS HAS TO EXIST otherwise the lifetime causes the program to crash
	sel_type: SelectionType,
	cur_song_path: String,
	cur_song_index: usize,
	songs_list: Vec<String>,
	search_text: String,
	error: String,
	volume: f32,
	start_system: SystemTime,
	start_milis: u64,
	position: u64,
	total_duration: u64,
	loopy: bool,
	current_song_info: SongInfo,
	dat_map: HashMap<String, String>,
}

impl Default for App {
	fn default() -> Self {
		let (i1, i2) = OutputStream::try_default().unwrap();
		let mut songls: Vec<String> = vec![];
		let paths = fs::read_dir("songs\\");
		let mut data_map: HashMap<String,String> = HashMap::new();

		initialize_data_map(&mut data_map);

		match paths {
			Ok(pat) => for p in pat {
				songls.clear();
				songls.push(format!("{}", format!("{}", 
					p.unwrap().path().to_str().unwrap().split("\\").collect::<Vec<&str>>().last().unwrap())
				));
			},
			Err(_) => {
				songls.clear();
				songls.push(format!("Error in fetching Music directory"));
			},
		}

		Self {
			_stream: i1,
			loopy: false,
			sink: rodio::Sink::try_new(&i2).unwrap(),
			sel_type: SelectionType::All,
			cur_song_path: format!("songs\\{}", songls.get(0).unwrap()),
			cur_song_index: 0,
			songs_list: songls,
			search_text: format!(""),
			error: format!(""),
			volume: 1.0,
			start_system: SystemTime::now(),
			total_duration: 0,
			start_milis: 0,
			position: 0,
			current_song_info: SongInfo::default(),
			dat_map: data_map,
		}
	}
}

impl eframe::App for App {
	fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
		ctx.request_repaint_after(Duration::from_secs(1));
		ctx.set_visuals(Visuals::dark());
		ctx.set_pixels_per_point(1.33);

		egui::CentralPanel::default().show(ctx, |ui| {
			ui.horizontal(|ui| {
				ui.heading("Songs");
				egui::ComboBox::from_label("")
					.selected_text(format!("{:?}", self.sel_type))
					.show_ui(ui, |ui| {
						ui.selectable_value(&mut self.sel_type, SelectionType::All, "All");
						ui.selectable_value(&mut self.sel_type, SelectionType::Artist, "Artist");
						ui.selectable_value(&mut self.sel_type, SelectionType::Song, "Song");
					}
				);
				ui.checkbox(&mut self.loopy, "Loop songs on finish");
			});
			ui.horizontal(|ui| {
				if ui.button("Get").clicked() {
					
					self.songs_list.clear();

					let paths = fs::read_dir("songs\\");
					match paths {
						Ok(pat) => {
							for p in pat {
								if let Ok(a) = p {
									self.songs_list.push(a.file_name().into_string().unwrap());
								}
							}
						},
						Err(_) => {
							self.songs_list.push(format!("Error in fetching Music directory"));
						},
					}
				}
				if ui.button("Shuffle").clicked() {
	
				}
				ui.add(egui::TextEdit::singleline(&mut self.search_text));
			});
			ui.add_space(10.0);
			ui.horizontal(|ui| {
				ui.set_min_height(200.0);
				ui.vertical(|ui| {
					egui::ScrollArea::vertical().show(ui, |ui| {
						ui.set_max_width(275.0);
						ui.set_min_width(275.0);
						let mut song_change_triggered = false;
						for (index, dir) in (&mut self.songs_list).into_iter().enumerate() {
							if self.search_text.len() != 0 {
								if dir.to_ascii_lowercase().contains(&self.search_text) {
									ui.horizontal(|ui| {
										ui.label(dir.clone());
										if ui.button(">>").clicked() {
											song_change_triggered = true;
											self.cur_song_index = index;
										}
									});
								}
							} else {
								ui.horizontal(|ui| {
									ui.label(dir.clone());
									if ui.button(">>").clicked() {
										song_change_triggered = true;
										self.cur_song_index = index;
									}
								});
							}
						}
						if song_change_triggered {
							let item = &self.songs_list.get(self.cur_song_index).unwrap();
							let fp = format!("songs\\{}", item);
							let file = File::open(&fp).unwrap();
							let map_data = self.dat_map.get(*item);

							if let Some(map_data) = map_data {
								let collection = map_data.split(',').collect::<Vec<&str>>();

								self.current_song_info.name = (**collection.get(1).unwrap()).to_string();
								self.current_song_info.artist = (**collection.get(2).unwrap()).to_string();
								self.current_song_info.genre = (**collection.get(3).unwrap()).to_string();
							}
		
							let reader = BufReader::new(file);
		
							self.error = play_song(self, reader, &fp);
						}
					});
				}); 
				
				ui.vertical(|ui| {
					ui.set_max_width(200.0);
					ui.vertical_centered(|ui| {
						ui.heading("Song Info");
					});
					ui.horizontal(|ui| {
						let song_label = ui.label("Song");
						ui.text_edit_singleline(&mut self.current_song_info.name).labelled_by(song_label.id);
					});
					ui.horizontal(|ui| {
						let artist_label = ui.label("Artist");
						ui.text_edit_singleline(&mut self.current_song_info.artist).labelled_by(artist_label.id);
					});
					ui.horizontal(|ui| {
						let genre_label = ui.label("Genre");
						ui.text_edit_singleline(&mut self.current_song_info.genre).labelled_by(genre_label.id);
					});
					if ui.button("Save").clicked() {
						save_data(&self.current_song_info, &mut self.dat_map,
								  &self.songs_list, 	 		self.cur_song_index);
					}
				});
			});
		});
		
		egui::TopBottomPanel::bottom("Player").show(ctx, |ui| {
			ui.horizontal(|ui| {
				ui.label(format!("Currently Playing: {}", self.songs_list.get(self.cur_song_index as usize).unwrap()));
				
				ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
					ui.label(&self.error)
				});
				
			});
			ui.horizontal(|ui| {
				if ui.button("Play").clicked() {
					let fp = format!("songs\\{}", self.songs_list.get(self.cur_song_index).unwrap());
					let open_file = File::open(&fp);

					if let Ok(open_file) = open_file {
						let reader = BufReader::new(open_file);
						
						self.error = play_song(self, reader, &fp);
					}
					else {
						self.error = format!("File not found: {}", &self.cur_song_path);
					}
				}
				match self.sink.is_paused() {
					true => if ui.button("Unpause").clicked() {self.sink.play(); self.start_system = SystemTime::now();},
					false => if ui.button("Pause").clicked() {self.sink.pause(); self.start_milis = self.position;},
				}
				
				if ui.button("Kill").clicked() {
					self.sink.skip_one();
				}
				
				let og_spacing = ui.spacing().slider_width;
				let size = ctx.available_rect().size().x - 353.0;
				ui.spacing_mut().slider_width = size;

				let secs = self.position / 1000;
				
				let seeker = ui.add(
					egui::Slider::new(&mut self.position, 0..=self.total_duration)
					.handle_shape(egui::style::HandleShape::Rect { aspect_ratio: 1.0 })
					.show_value(false)
					.text(format!("{}:{}{}", secs / 60, if secs % 60 < 10 {"0"} else {""}, secs % 60))
				);
				ui.spacing_mut().slider_width = og_spacing;
				
				// This is to prevent an issue that would cause an infinite loop somehow
				if seeker.dragged() {
					let _ = self.sink.try_seek(Duration::from_millis(self.position));
					self.start_system = SystemTime::now();
					self.start_milis = self.position;
				} else {
					if self.sink.empty() {
						let fp = format!("songs\\{}", self.songs_list.get(self.cur_song_index).unwrap());
						let open_file = File::open(&fp);
	
						if self.loopy {
							if let Ok(open_file) = open_file {
								let reader = BufReader::new(open_file);
								
								self.error = play_song(self, reader, &fp);
							}
							else {
								self.error = format!("File not found: {}", &self.cur_song_path);
							}
						}
					}
				}
				if self.position < self.total_duration && !self.sink.is_paused() && !self.sink.empty() {
					self.position = self.start_system.elapsed().unwrap().as_millis() as u64 + self.start_milis;
				}
				
				ui.with_layout(egui::Layout::right_to_left(egui::Align::RIGHT), |ui| {
					let volume_slider = ui.add(egui::Slider::new(&mut self.volume, 0.0..=1.0));
					if volume_slider.dragged() {
						self.sink.set_volume(self.volume);
					}
				});
			});
		});
	}
}

fn play_song(app: &mut App, reader: BufReader<File>, fp: &str) -> String {
	let elem = rodio::Decoder::new_mp3(reader);
	match elem {
		Ok(a) => {
			let path = Path::new(&fp);
			let path_test = mp3_duration::from_path(&path);
			if let Ok(path_test) = path_test {
				app.total_duration = path_test.as_millis() as u64;
			} else {
				let t = a.total_duration();
				if let Some(t) = t {
					app.total_duration = t.as_millis() as u64;
				} else {
					return format!("Error - Couldn't determine song length");
				}
			}
			app.total_duration = mp3_duration::from_path(&path).unwrap().as_millis() as u64;
			app.sink.stop();

			app.start_system = SystemTime::now();
			app.position = 0;
			app.start_milis = 0;

			app.sink.append(a); 
			format!("")},
		Err(_) => format!("Error in decoding song :("),
	}
}

fn save_data(current_song_info: &SongInfo, dat_map: &mut HashMap<String, String>, songs_list: &Vec<String>, cur_song_index: usize) {
	let current_s = songs_list.get(cur_song_index).unwrap();
	let data = format!("{},{},{},{},{}", current_s, current_song_info.name, current_song_info.artist, current_song_info.genre, current_song_info.nodisplay_time_listened);
	
	dat_map.insert(current_s.clone(), data);
	fs::write("data.csv", "").expect("Unable to write file");

	for keys in dat_map.keys() {
		let mut f = OpenOptions::new()
			.append(true)
			.open("data.csv")
			.unwrap();
		let _ = writeln!(f, "{}", dat_map.get(keys).unwrap()).is_ok();
	}
}

fn initialize_data_map(data_map: &mut HashMap<String,String>) {
	let fp = format!("data.csv");
	let file = File::open(&fp);

	if let Ok(file) = file {
		let reader = BufReader::new(file);
		for line in reader.lines() {
			let unwrapped_line = line.unwrap();
			let unw_clone = unwrapped_line.clone();
			let collection = unwrapped_line.split(',').collect::<Vec<&str>>();
	
			let key = (**collection.get(0).unwrap()).to_string();

			data_map.insert(key, unw_clone);
		}
	}
}