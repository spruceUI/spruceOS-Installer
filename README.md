# spruceOS-Installer

spruceOS-Installer

This is an all in one downloader, extracter, formater, installer for made for spruce!

It can be edited easily and be made to work for any othe CFW that requires files copied onto a fat32 sd card with little to no struggle.

Githuib actions are set up run run and create releases by branch. If you would like to use this program let us know and we can create a branch for you (or just add you to this repo directly).

Please do not delete the spruce team from the authors section; instead add your name to the listing in addition. 



To Do:

~~List supported devices to version description \& widem drop doen to accomodate the longer message~~ NVMD this will have to happen some other way

* Clean up colors to match SPRUCE theme.





  To rebrand the installer, users need to:



  1. In src/config.rs:

    - Change APP\_NAME (e.g., "MuOS")

    - Change VOLUME\_LABEL (e.g., "MUOS")

    - Change REPO\_OPTIONS to their GitHub repos

    - Change the color values to match your project


  2. In Cargo.toml:

    - Change name (e.g., "Your Name-installer")

    - Change description

  3. In assets/Mac/Info.plist:

    - Change CFBundleName, CFBundleDisplayName, CFBundleIdentifier

  4. Replace icons:

    - assets/Icons/icon.png

    - assets/Icons/icon.ico

