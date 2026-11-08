# Demo of the README, recorded with VHS (https://github.com/charmbracelet/vhs)
# into assets/demo.gif by the `demo` job of .github/workflows/ci.yml:
#
#   cargo build --release --locked && PATH="$PWD/target/release:$PATH" vhs demo.tape
#
# A short run of the seed 9940 in an 80 x 24 terminal: the title screen, the
# seed typed in, a walk to a glyph that teaches the sigil Ember, Ember traced
# on the grid (7, 5, 3) and cast at a rat, a melee fight, then the stairs down
# to the second floor. The game is deterministic for a seed and VHS types the
# same keys every time, so every recording plays the same run.
#
# The keys typed after `sigilfall` are replayed by the test
# `app::tests::demo_tape` of the sigilfall crate, which checks what this run
# shows; change them only with that test green. Only Type, Enter, Escape,
# Backspace, Space, Tab, the arrows, Ctrl+C and Sleep may be used after the
# game starts.

Output assets/demo.gif

Require sigilfall

Set Shell "bash"
Set FontFamily "JetBrains Mono"
Set FontSize 20
# Room for a little more than 80 x 24 cells; `stty` below gives the game
# exactly 80 x 24, and the margin takes the colour of the game's background.
Set Width 1060
Set Height 680
Set Padding 20
Set Framerate 30
Set TypingSpeed 90ms
# The dark theme of the game (theme.rs): ink background, bone text.
Set Theme { "name": "Sigilfall", "background": "#0F0D15", "foreground": "#E6DECB", "cursor": "#F5C451", "selection": "#2E2742", "black": "#0F0D15", "red": "#EC6A50", "green": "#76C891", "yellow": "#E3B24F", "blue": "#8DBDE6", "magenta": "#B794F0", "cyan": "#5ED6B8", "white": "#E6DECB", "brightBlack": "#6E6684", "brightRed": "#F07D63", "brightGreen": "#76C891", "brightYellow": "#F5C451", "brightBlue": "#8DBDE6", "brightMagenta": "#B794F0", "brightCyan": "#5ED6B8", "brightWhite": "#F2EADB" }

# A fresh, empty data folder, so no save or options file of an earlier run
# changes the title screen; the 24-bit colours of the game; the size of the
# smallest terminal the game supports.
Hide
Type "export SIGILFALL_DATA_DIR=$(mktemp -d) COLORTERM=truecolor && stty cols 80 rows 24 && clear"
Enter
Sleep 500ms
Show

Type "sigilfall"
Sleep 600ms
Enter
Sleep 2.5s

# Title menu: down to "Seed", then type the seed of the run.
Down
Sleep 500ms
Enter
Sleep 400ms
Type@150ms "9940"
Sleep 800ms
Enter
Sleep 2s

# Walk to the glyph carved in the first room: stepping on it reads it, and
# Ember joins the grimoire.
Type@110ms "bbbbyybbyyyyykk"
Sleep 2s

# Up the corridor, across the next room and along the passage that leads
# to the stairs, until a rat comes into sight.
Type@110ms "kkkyhhhhhyyyyykkkkkullllllll"
Sleep 1.5s

# Trace Ember (7, 5, 3) on the grid and cast it at the rat.
Type "t"
Sleep 1.2s
Type@500ms "753"
Sleep 1.2s
Enter
Sleep 2s

# On to the stairs, through a wax beetle that has to be fought in melee.
Type@130ms "llllllyyyyyykkk"
Sleep 1.5s

# Down to the second floor.
Type ">"
Sleep 3s

# Save and quit: the run waits in the temporary data folder.
Type "q"
Sleep 1s
