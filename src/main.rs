#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use eframe::{egui, egui::Visuals};
use std::{fs::{self, File}, io::BufReader, str, time::Duration};
use rodio::{OutputStream, OutputStreamHandle};

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([690.0, 320.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Dreamer",
        options,
        Box::new(|cc| {Box::<App>::default()}),
    )
}

#[derive(PartialEq)]
#[derive(Debug)]
enum SelectionType {All,Artist,Song}

struct App {
    sink: rodio::Sink,
    _stream: OutputStream,
    stream_handle: OutputStreamHandle,
    sel_type: SelectionType,
    cur_song_path: String,
    songs_list: Vec<String>,
    error: String,
}

impl Default for App {
    fn default() -> Self {
        let (i1, i2) = OutputStream::try_default().unwrap();
        let mut songls: Vec<String> = vec![];
        let paths = fs::read_dir("songs\\");
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
            sink: rodio::Sink::try_new(&i2).unwrap(),
            stream_handle: i2,
            sel_type: SelectionType::All,
            cur_song_path: format!("songs\\{}", songls.get(0).unwrap()),
            songs_list: songls,
            error: format!(""),
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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
                if ui.button("Shuffle").clicked() {

                }
            });
            if ui.button("Get").clicked() {
                let paths = fs::read_dir("songs\\");
                match paths {
                    Ok(pat) => {
                    self.songs_list.clear();
                    for p in pat {
                        if let Ok(a) = p {
                            println!("Pushing {}", a.file_name().into_string().unwrap());
                            self.songs_list.push(a.file_name().into_string().unwrap());
                        }
                    }
                    },
                    Err(_) => {
                        self.songs_list.clear();
                        self.songs_list.push(format!("Error in fetching Music directory"));
                    },
                }
            }
            let mut i = 0;
            for dir in &self.songs_list {
                ui.horizontal(|ui| {
                    ui.label(dir);
                    if ui.button(">>").clicked() {
                        let file = BufReader::new(File::open(format!("songs\\{}", self.songs_list.get(i).unwrap())).unwrap());
                        let elem = rodio::Decoder::new(file);
                        self.error = match elem {
                            Ok(a) => {self.sink.append(a); format!("")},
                            Err(_) => format!("Error in decoding song :("),
                        }
                    }
                });
                i += 1;
            }
            ui.label(&self.error);
        });
        
        egui::TopBottomPanel::bottom("Player").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Play").clicked() {
                    let open_file = File::open(&self.cur_song_path);

                    if let Ok(o) = open_file {
                        let file = BufReader::new(o);

                        let elem = rodio::Decoder::new(file);
                        self.error = match elem {
                            Ok(a) => {self.sink.append(a); format!("")},
                            Err(_) => format!("Error in decoding song :("),
                        }
                    }
                    else {
                        self.error = format!("File not found: {}", &self.cur_song_path);
                    }
                }
                match self.sink.is_paused() {
                    true => if ui.button("Unpause").clicked() {self.sink.play();},
                    false => if ui.button("Pause").clicked() {self.sink.pause();},
                }

                if ui.button("Kill").clicked() {self.sink.stop();}
            });
        });
    }
}