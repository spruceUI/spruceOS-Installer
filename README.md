# spruceOS-Installer

spruceOS-Installer

This is an all in one downloader, extracter, formater, installer for spruce!

Hopefully it can be edited and be made to work for any othe CFW that requires files copied onto a fat32 sd card with little to no struggle.



To Do:

* Add a cancel button
* Extract to pc, THEN copy to drive???

~~List supported devices to version description \& widem drop doen to accomodate the longer message~~ NVMD this will have to happen some other way

* Clean up colors to match SPRUCE theme.
* Fix the shown % for extraction + copying files (if we go this route).





  To rebrand the installer, users need to:



  1. In src/config.rs:

    - Change APP\_NAME (e.g., "MuOS")

    - Change VOLUME\_LABEL (e.g., "MUOS")

    - Change REPO\_OPTIONS to their GitHub repos

&nbsp;   - Change the color values

  2. In Cargo.toml:

    - Change name (e.g., "Your Name-installer")

    - Change description

  3. In assets/Mac/Info.plist:

    - Change CFBundleName, CFBundleDisplayName, CFBundleIdentifier

  4. Replace icons:

    - assets/Icons/icon.png

    - assets/Icons/icon.ico

