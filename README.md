This is a lightweight, no-bs mp3 player written in Rust.

Right now it may not be the most user-friendly, because I built it originally just for myself without other considerations. Currently working to make it a bit more accessible, and have more features that everyone will find useful!
## TODO:
### Polish
- Come up with an actual name lol
- Create an exe icon
- Limit the window resizing capability
- **\[DONE\]** Add a "saved successfully" text upon successful save
	- Now make it disappear after a bit
- **\[DONE\]** Add a warning for when a song doesn't have any saved data associated with it
- **\[FIXED\]** Either fix the initial getting of the song list, or disable it entirely
- **\[DONE\]** Remove dead and/or useless ui components
- Add tooltips
- **\[DONE\]** Bold currently playing song
- **\[DONE\]** Add greyed out text to search box when empty
- **\[DONE\]** Make volume slider logarithmic
- **\[DONE\]** Default volume to 0.5
- **\[DONE\]** Make song timer refresh more often
- **\[DONE\]** Add long text truncation
### Bugfixes
- **\[FIXED\]** Allow song looping while minimized (This was so hard oh my god)
- **\[FIXED\]** When refreshing the song list, update the song ID for the currently playing song
- **\[FIXED\]** Fix searching breaking when typing capital letters
- Add broader support for non-ascii characters (Unlikely to fix. This could be very difficult)
- **\[FIXED\]** Fix data not being grabbed when song first loaded.
- **\[FIXED\]** Fix desync between timer and position within the song when dragging to the end
- **\[MOSTLY FIXED\]** There's a lot of `unwrap()` calls left that need to be handled/vetted.
	- Note: There are still like 40, but most I will not be handling. There's a comment at the top of main.rs that explains why.
### Features
- Allow song grouping into folders
- **\[ADDED\]** Create shuffle feature and in-order playing
- Maybe add speeding up/slowing down of audio?
### Optimization
- **\[DONE\]** Cache search results into a separate vec to avoid having to linear search each redraw
- **\[DONE\]** Optimize large scroll areas (somehow??? (nvm it was actually really easy))
### Other
- Create a user manual/documentation
- Make a download page
