# MacOS Media Presence
This is a personal project that was made to practice using rust
If you want to use this yourself, you will have to change a few things and have the [media-control](https://github.com/ungive/media-control) CLI installed.

1. In the ```get_stream``` function, add or change the if statement that uses ```text.bundleIdentifier``` to use something other than ```"com.foobar2000"``` if you want it to be able to detect medias other than foobar2000. (Or just remove the if statement if you want all media to be detected. This will include youtube videos, literally everything pretty much). The bundle identifier can be read with ```$ media-control get```
2. In the ```post``` function, you might want to change the post url to be a different image hosting service. If you do this, also remove the if statement at line 255.
3. Be using MacOS. I don't like it, but I can't exactly just get a new laptop lol.
