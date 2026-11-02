# Changelog

What changed in each Couchlink release. Downloads and older notes are on the
[releases page](https://github.com/cranebeaker/couchlink/releases).

## 0.1.14

### New

- **Start from other apps.** A new page in the sidebar has the launch command for every installed game, with steps
  and copy buttons for Steam, Playnite and LaunchBox. `couchlink.exe --play <game>` starts the game with the players
  and screens set up in Couchlink, and starts Couchlink first when it is not running.
  - **Steam:** "Add to Steam" puts the game in your library as a non-Steam game (Steam closes and opens again to show
    it; your other non-Steam games stay, and the old list is kept as a backup). Press Play in Steam, Big Picture too.
  - **LaunchBox:** add Couchlink once as an emulator with `--play-exe`, and every game on that platform opens through
    Couchlink by itself.
  - **Desktop shortcut:** one button, with the game's own icon.
- **Steam per game.** Each game's page has Steam: Automatic, Needed or Not needed. Needed opens your Steam and every
  extra player's own Steam when a session starts, and waits until they are signed in. Automatic picks Needed for
  Steam games and games with online-fix, which now also say so on their page.

## 0.1.13

### New

- **Steam's overlay (Shift+Tab) in games on real Steam.** Couchlink starts each copy itself, so Steam never put
  its overlay in. Now each copy loads Steam's own overlay, also for players on their own Steam account, so Shift+Tab
  opens friends, invites and achievements for that player. On by default; each game's page has a switch to turn it
  off. Games on the Steam emulator have the same switch for its overlay.
- **Saved layouts.** In the screen editor, "Save layout" keeps the screen as it is, gaps included. Keep as many as
  you like and put any of them on a screen in any game. A game's page can pin one, so that game always starts with
  it.
- **Close a player's own Steam.** Players, a player's Steam account: "Close their Steam". Settings, General:
  "Close their Steam when a session ends". Your own Steam stays open.
- **Show a cursor for each player** (Settings, Bindings): turn off Couchlink's extra cursor for games that draw their
  own.
- **Keyboards and mice only control the games while a game is in front** (Settings, Bindings, on by default):
  typing in Discord or another app no longer reaches the games.

### Fixed

- **A game lost frames while another one was in front.** Windows gives the window in front more of the PC.
  Couchlink already took the focus away from the games at the start; now it does so again whenever a game copy is
  clicked into, so every copy runs alike. Not when someone plays on the desktop's own keyboard and mouse.
- **Couchlink's cursor showed on top of other apps** after switching away from the game. It now stays with its game
  window.
- **Checking whether a player's own Steam runs took up to a minute** on some PCs, which also slowed starting games
  with it. It is instant now.

## 0.1.12

### Fixed

- **Games with online-fix play on each player's own Steam account.** A game whose folder has online-fix
  (`OnlineFix64.dll` or `OnlineFix.ini`) talks to the Steam running on the PC, so before every copy ended up on
  player 1's account. Couchlink now treats such a game like a Steam game: every player who has their own Steam
  account set up (Players, "Own Steam account") plays on it, under the app id online-fix names.

## 0.1.11

### Fixed

- **Changing keyboards and mice while games run.** Moving a keyboard or mouse to another player (or swapping
  players) in the hub now reaches the running copies straight away, in every game. Before, the copies kept the
  devices they started with. A player whose copy started without any keyboard or mouse of their own gets the new one
  from the next start; the hub says so. Controllers still keep the ones they started with. Windows only for now.
- **Linux: Proton games find their settings.** For a Windows game that runs through Proton, a recipe's Documents
  and AppData folders are now the ones in the game's Steam prefix, where the game keeps its settings, instead of the
  home folder. This fixes the Fallout: New Vegas co-op recipe saying its setup is not done on Linux.
- **Linux: each player's copy starts with the game's settings.** Every player's own Proton prefix now starts with a
  copy of the Documents, AppData and Saved Games folders from the game's Steam prefix (once, folders up to 1 GB),
  so games that need files there no longer start without them.
- **Linux: signing in to Steam for player 2 and up.** Each extra player's Steam now gets its own user name and its
  own config, data, state and cache folders, which should stop it looping between "setting up runtime" and
  "updating" and then closing.

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
- **Start without the recipe.** Every game with a recipe now also offers a plain split screen: the game's own program,
  one copy per player in borderless windows, without the recipe's mods or setup. Use it with another co-op mod you
  installed yourself (for example a different New Vegas co-op mod), or to play split screen without one. It works
  before the recipe's setup is done too.
- **Nitrox finds more installs.** The Subnautica (Nitrox) recipe looks in more folders, and when it cannot find
  Nitrox it says where to unzip it and to point it at the same Subnautica folder.
- **Make your own recipe.** A new tab under "Find more" has a prompt to copy into an AI coding agent. It has the agent
  look at the game on your PC, write a recipe for it and check it, in the folder Couchlink reads recipes from.

### Good to know

- Third-party tools Couchlink downloads are credited in `hook/THIRD_PARTY.md`: the Virtual Display Driver and nefcon
  (used once to install it) join the existing credits.
