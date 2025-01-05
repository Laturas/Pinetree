use std;

#[derive(PartialEq)]
pub enum FileType {
    AudioFile, Directory
}

pub struct FileElement {
    pub file_type: FileType,
    pub file_name: String,
}

impl FileElement {
    pub fn new(file_type: FileType, file_name: String) -> Self {
        Self {
            file_name: file_name,
            file_type: file_type,
        }
    }
}

pub struct FileTreeNode {
    folder_name: String,
    subfolders: Vec<String>,
    songs: Vec<String>,
}

impl FileTreeNode {
    /// Automatically gets all the songs and subfolders set up
    /// Does NOT set up the subsoundtrees
    pub fn new(directory_file_path: String) -> Self {
        let mut songs_list: Vec<String> = Vec::new();
        let mut subfolder_list: Vec<String> = Vec::new();

        let paths = if directory_file_path.len() == 0 {
            // For some reason it doesn't read the current directory when given an empty string. Strange imo but whatever
            std::fs::read_dir("./")
        } else {
            std::fs::read_dir(&directory_file_path)
        };
	
        if let Ok(paths) = paths {
            for p in paths {
                if let Ok(a) = p {
                    // I tried to figure out where this .file_type() method could possibly fail, but I have no idea. Maybe platform dependence?
                    if a.file_type().unwrap().is_dir() {
                        // If the file names contain invalid unicode data it's best to just ignore them
                        if let Ok (fname) = a.file_name().into_string() {
                            subfolder_list.push(fname);
                        }
                    }
                    else {
                        let song_result = a.file_name().into_string();
                        if let Ok(song_result) = song_result {
                            if song_result.ends_with(".mp3") {
                                songs_list.push(song_result);
                            }
                        }
                    }
                    
                }
            }
        }
        // Rare windows W
        // Windows returns the files sorted alphabetically. But this is platform dependent behavior
        // To keep songs in order on non-windows platforms we run a sort() call.
        if !cfg!(windows) {
            songs_list.sort();
        }

        Self {
            folder_name: directory_file_path,
            subfolders: subfolder_list,
            songs: songs_list,
        }
    }
}

use std::collections::HashMap;

/// Walks the directory tree starting at root_name using a DFS and outputs the result to out_vec
/// Deletes any previous information stored in out_vec
pub fn walk_tree(out_vec: &mut Vec<FileElement>, root_name: &str, hashmap: &HashMap<String, FileTreeNode>) {
    out_vec.clear();
    let result = hashmap.get(root_name);
    if let Some(root_node) = result {
        out_vec.push(FileElement::new(FileType::Directory, root_name.to_owned()));

        for dir in &root_node.subfolders {
            out_vec.push(FileElement::new(FileType::Directory, dir.to_owned()));
            if hashmap.contains_key(dir) {
                walk_tree_recursive(out_vec, &file_path_build(&root_name, &dir), &hashmap);
            }
        }
        for song in &root_node.songs {
            out_vec.push(FileElement::new(FileType::AudioFile, file_path_build(root_name, &song)));
        }
    }
}

fn walk_tree_recursive(out_vec: &mut Vec<FileElement>, root_name: &str, hashmap: &HashMap<String, FileTreeNode>) {
    let result = hashmap.get(root_name);
    if let Some(root_node) = result {

        for dir in &root_node.subfolders {
            out_vec.push(FileElement::new(FileType::Directory, dir.to_owned()));
            if hashmap.contains_key(dir) {
                walk_tree_recursive(out_vec, &file_path_build(&root_name, &dir), &hashmap);
            }
        }
        for song in &root_node.songs {
            out_vec.push(FileElement::new(FileType::AudioFile, file_path_build(root_name, &song)));
        }
    }
}

pub fn file_path_build(folder_paths: &str, file_name: &str) -> String {
	if folder_paths.ends_with('/') || folder_paths.ends_with('\\') {
		return format!("{}{}", folder_paths, file_name);
	} else {
		return format!("{}/{}", folder_paths, file_name);
	}
}