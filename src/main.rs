#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use eframe::{egui, egui::Visuals};
use egui::{epaint::PathShape, util::id_type_map::TypeId};
use std::{fs::{self, canonicalize, File}, io::{BufReader, Write}, path::Path, process::Command, str, sync::mpsc::Receiver, thread::{self, JoinHandle}, time::Duration};
use std::sync::mpsc;
use rodio::{source::Source, Decoder, OutputStream, OutputStreamHandle};

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
            cur_song_path: format!("{}", songls.get(0).unwrap()),
            songs_list: songls,
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
                    Ok(pat) => for p in pat {
                        self.songs_list.clear();
                        self.songs_list.push(format!("{}", format!("{}", 
                            p.unwrap().path().to_str().unwrap().split("\\").collect::<Vec<&str>>().last().unwrap())
                        ));
                    },
                    Err(_) => {
                        self.songs_list.clear();
                        self.songs_list.push(format!("Error in fetching Music directory"));
                    },
                }
            }
            for dir in &self.songs_list {
                ui.horizontal(|ui| {
                    ui.label(dir);
                    if ui.button(">>").clicked() {
                        let file = BufReader::new(File::open(format!("songs\\{}", self.songs_list.get(0).unwrap())).unwrap());
                        self.sink.append(rodio::Decoder::new(file).unwrap());
                    }
                });
            }

        });
        
        egui::TopBottomPanel::bottom("Player").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Play").clicked() {
                    //println!("{}", self.cur_song_path);
                    let file = BufReader::new(File::open(&self.cur_song_path).unwrap());
                    self.sink.append(rodio::Decoder::new(file).unwrap());
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

fn playtrack(path: &str) {

}