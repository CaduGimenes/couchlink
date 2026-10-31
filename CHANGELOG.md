# Changelog

What changed in each Couchlink release. Downloads and older notes are on the
[releases page](https://github.com/cranebeaker/couchlink/releases).

## 0.1.10

### New

- **Virtual screens.** In the screen editor, "Show games as" now offers Virtual screens (most compatible) next to
  Windows (fastest). Each player gets a virtual monitor sized exactly to their part of the screen, so games that only
  run full screen play properly in split screen, at the right size. Couchlink shows each virtual monitor live in its
  player's part. It adds about a frame of delay. Windows only.
  - The first time, the screen editor offers to install the free Virtual Display Driver (MIT). It is one Windows
    prompt; the download is checked before it is installed.
  - The virtual monitors exist only while a session runs. Afterwards, and after a crash, the driver's settings are put
    back exactly as they were, and the virtual monitors never show up as screens to put players on.
- **Host control.** Keyboards and mice no player has now work in whichever game the pointer is over, for example to
  change a game's settings. Turn it off in Settings if you prefer. Takes effect from the next start.
- **Take every keyboard and mouse.** A new shortcut (Ctrl+Shift+K by default, changeable or off in Settings) hands
  every keyboard and mouse to the game under the pointer. Press it again to give them back to their players.
- **Make your own recipe.** A new tab under "Find more" has a prompt to copy into an AI coding agent. It has the agent
  look at the game on your PC, write a recipe for it and check it, in the folder Couchlink reads recipes from.

### Good to know

- Third-party tools Couchlink downloads are credited in `hook/THIRD_PARTY.md`: the Virtual Display Driver and nefcon
  (used once to install it) join the existing credits.
