This is a lightweight, no-bs mp3 player written in Rust.

Right now it may not be the most user-friendly, because I built it originally just for myself without other considerations. Currently working to make it a bit more accessible, and have more features that everyone will find useful!
## TODO:
### Polish
- Come up with an actual name lol
- Create an exe icon
- Limit the window resizing capability
- Add a "saved successfully" text upon successful save
- Add a warning for when a song doesn't have any saved data associated with it
### Bugfixes
- Allow song looping while minimized
- When refreshing the song list, update the song ID for the currently playing song
- Fix searching breaking when typing capital letters
### Features
- Allow song grouping into folders
- Create shuffle feature and in-order playing
### Optimization
- Cache text-rendering results to reduce CPU usage
	- (Honestly that's the only real thing that would really benefit from being optimized, imo)
### Other
- Create a user manual/documentation
