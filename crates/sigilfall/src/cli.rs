//! The command line: options the game is started with.
//!
//! Exposes [`Cli`], read from the process arguments by clap, which also
//! generates `--help` and `--version`. The help is in English whatever the
//! language of the game, as the help of command-line tools usually is: it
//! is printed before any option is read, and names options that are
//! English words. The help is wrapped to the width of
//! the terminal (clap's `wrap_help` feature; when the output is not a
//! terminal, to `COLUMNS`, else 100 columns), so it reads whole in an
//! 80-column terminal. An invalid value, such as `--seed abc` or `--seed -1`,
//! is reported by clap with the accepted forms of the value and ends the
//! process with a non-zero exit code, before the terminal is touched. So is
//! `--daily` given with `--seed`: the daily dungeon has its own seed, so the
//! two cannot both choose the run. `--lang` and `--keyboard` choose the
//! language and the keyboard layout for one session, without changing the
//! options.

use clap::Parser;
use sigilfall_core::rng::Seed;

use crate::keyboard::Keyboard;
use crate::language::Language;

/// Terminal roguelike where spells are cast by tracing sigils on the
/// keyboard.
#[derive(Debug, Clone, PartialEq, Eq, Parser)]
#[command(version, about)]
pub struct Cli {
    /// Seed of the run, to replay or share a dungeon: a number such as 42,
    /// or the seed shown in the game, such as 7F3A-91C2-0B5E-D4A8. The game
    /// starts at once, without the title screen. Refused while a game is
    /// saved, so that game is never lost: continue or abandon it from the
    /// title screen first.
    // A value that looks like a negative number, such as `--seed -1`, is
    // taken as the seed, so it is refused with the accepted forms instead of
    // being reported as an unknown option.
    #[arg(long, value_name = "SEED", allow_negative_numbers = true)]
    pub seed: Option<Seed>,

    /// Play the daily dungeon: the same for every player on the same UTC
    /// date, and a new one each day at midnight UTC. The game starts at
    /// once, without the title screen. Cannot be given with --seed, and
    /// refused while a game is saved, as --seed is.
    #[arg(long, conflicts_with = "seed")]
    pub daily: bool,

    /// Draw with plain ASCII characters instead of Unicode glyphs, for fonts
    /// that lack them or terminals that draw them on two cells. Applies to
    /// this session only, whatever the glyph option says.
    #[arg(long)]
    pub ascii: bool,

    /// Language of the game for this session, whatever the Language option
    /// says: en (English) or fr (French). Without it, the game speaks the
    /// language chosen in the options, or else the system's.
    #[arg(long, value_name = "LANG")]
    pub lang: Option<Language>,

    /// Keyboard layout for this session, whatever the Keyboard option says:
    /// qwerty (WASD to move) or azerty (ZQSD to move, the top row without
    /// Shift for the digits). Without it, the game follows the layout chosen
    /// in the options, or else the one usual with the system's language.
    #[arg(long, value_name = "LAYOUT")]
    pub keyboard: Option<Keyboard>,
}

/// Returns a new random seed, for a new game started without a seed.
///
/// The seed comes from `rand::random`, whose generator is seeded by the
/// operating system's entropy source (through `getrandom`), so two games
/// started this way get different dungeons.
pub fn random_seed() -> Seed {
    Seed::new(rand::random())
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory as _;
    use clap::error::ErrorKind;

    use super::*;

    /// Parses `arguments`, given without the program name.
    fn parse(arguments: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("sigilfall").chain(arguments.iter().copied()))
    }

    #[test]
    fn the_command_line_definition_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn the_help_fits_an_80_column_terminal() {
        /// Width of the smallest terminal the game supports.
        const COLUMNS: usize = 80;

        let mut command = Cli::command().term_width(COLUMNS);
        let long = command.render_long_help().to_string();
        let short = command.render_help().to_string();

        for help in [&long, &short] {
            for line in help.lines() {
                assert!(line.chars().count() <= COLUMNS, "{line:?} in\n{help}");
            }
        }
        // Wrapped, not cut: the end of the longest description is still there.
        let words = long.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            words.contains("abandon it from the title screen first"),
            "{long}"
        );
    }

    #[test]
    fn no_argument_gives_no_seed_and_no_ascii() {
        let cli = parse(&[]).unwrap();

        assert_eq!(cli.seed, None);
        assert!(!cli.daily);
        assert!(!cli.ascii);
    }

    #[test]
    fn daily_is_read_alone_or_with_ascii() {
        let cli = parse(&["--daily"]).unwrap();
        assert!(cli.daily);
        assert_eq!(cli.seed, None);

        let expected = Cli {
            seed: None,
            daily: true,
            ascii: true,
            lang: None,
            keyboard: None,
        };
        assert_eq!(parse(&["--daily", "--ascii"]).unwrap(), expected);
        assert_eq!(parse(&["--ascii", "--daily"]).unwrap(), expected);
    }

    #[test]
    fn daily_and_seed_are_refused_together_whatever_their_order() {
        for arguments in [
            &["--daily", "--seed", "42"][..],
            &["--seed", "42", "--daily"],
            &["--seed=7F3A-91C2-0B5E-D4A8", "--ascii", "--daily"],
        ] {
            let error = parse(arguments).unwrap_err();
            let message = error.to_string();

            assert_eq!(error.kind(), ErrorKind::ArgumentConflict, "{message}");
            assert_ne!(error.exit_code(), 0, "{message}");
            assert!(message.contains("--daily"), "{message}");
            assert!(message.contains("--seed"), "{message}");
            assert!(message.contains("cannot be used with"), "{message}");
        }
    }

    #[test]
    fn daily_takes_no_value() {
        let error = parse(&["--daily=2026-11-08"]).unwrap_err();

        assert_ne!(error.exit_code(), 0, "{error}");
    }

    #[test]
    fn random_seeds_differ() {
        // Two draws of 64 random bits are equal once in 2^64 runs.
        assert_ne!(random_seed(), random_seed());
    }

    #[test]
    fn a_decimal_seed_is_used_as_given() {
        let cli = parse(&["--seed", "42"]).unwrap();

        assert_eq!(cli.seed, Some(Seed::new(42)));
    }

    #[test]
    fn the_seed_shown_in_the_game_is_accepted_back() {
        let shown = Seed::new(0x7F3A_91C2_0B5E_D4A8).to_string();

        let cli = parse(&["--seed", &shown]).unwrap();

        assert_eq!(cli.seed, Some(Seed::new(0x7F3A_91C2_0B5E_D4A8)));
        assert_eq!(parse(&[&format!("--seed={shown}")]).unwrap(), cli);
    }

    #[test]
    fn an_invalid_seed_is_refused_with_the_expected_format() {
        for value in ["abc", "4.2", "18446744073709551616", "7F3A-91C2"] {
            let error = parse(&["--seed", value]).unwrap_err();
            let message = error.to_string();

            assert_eq!(error.kind(), ErrorKind::ValueValidation, "{message}");
            assert_ne!(error.exit_code(), 0, "{message}");
            assert!(message.contains("--seed"), "{message}");
            assert!(message.contains(&format!("'{value}'")), "{message}");
            assert!(message.contains("such as 42"), "{message}");
            assert!(message.contains("7F3A-91C2-0B5E-D4A8"), "{message}");
        }
    }

    #[test]
    fn a_negative_seed_is_refused_with_the_expected_format() {
        for arguments in [&["--seed", "-1"][..], &["--seed=-1"], &["--seed", "-4.2"]] {
            let error = parse(arguments).unwrap_err();
            let message = error.to_string();

            assert_eq!(error.kind(), ErrorKind::ValueValidation, "{message}");
            assert!(message.contains("such as 42"), "{message}");
            assert!(message.contains("7F3A-91C2-0B5E-D4A8"), "{message}");
        }
    }

    #[test]
    fn an_option_after_seed_is_not_taken_as_its_value() {
        let error = parse(&["--seed", "--ascii"]).unwrap_err();

        assert_ne!(error.kind(), ErrorKind::ValueValidation, "{error}");
        assert_ne!(error.exit_code(), 0, "{error}");
    }

    #[test]
    fn a_missing_seed_value_is_refused() {
        let error = parse(&["--seed"]).unwrap_err();

        assert_ne!(error.exit_code(), 0, "{error}");
    }

    #[test]
    fn ascii_is_read_alone() {
        let cli = parse(&["--ascii"]).unwrap();

        assert!(cli.ascii);
        assert_eq!(cli.seed, None);
        assert!(!cli.daily);
    }

    #[test]
    fn options_combine_in_any_order() {
        let expected = Cli {
            seed: Some(Seed::new(7)),
            daily: false,
            ascii: true,
            lang: None,
            keyboard: None,
        };

        assert_eq!(parse(&["--ascii", "--seed", "7"]).unwrap(), expected);
        assert_eq!(parse(&["--seed", "7", "--ascii"]).unwrap(), expected);
    }

    #[test]
    fn keyboard_takes_qwerty_or_azerty_and_nothing_else() {
        assert_eq!(parse(&[]).unwrap().keyboard, None);
        assert_eq!(
            parse(&["--keyboard", "azerty"]).unwrap().keyboard,
            Some(Keyboard::Azerty)
        );
        assert_eq!(
            parse(&["--keyboard=qwerty"]).unwrap().keyboard,
            Some(Keyboard::Qwerty)
        );
        let error = parse(&["--keyboard", "qwertz"]).unwrap_err();
        let message = error.to_string();
        assert_eq!(error.kind(), ErrorKind::InvalidValue, "{message}");
        assert!(
            message.contains("qwerty") && message.contains("azerty"),
            "{message}"
        );
    }

    #[test]
    fn unknown_options_are_refused() {
        let error = parse(&["--graine", "42"]).unwrap_err();

        assert_eq!(error.kind(), ErrorKind::UnknownArgument);
        assert_ne!(error.exit_code(), 0);
    }
}
