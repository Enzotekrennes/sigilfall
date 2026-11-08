//! The command line: options the game is started with.
//!
//! Exposes [`Cli`], read from the process arguments by clap, which also
//! generates `--help` and `--version`. An invalid value, such as
//! `--seed abc` or `--seed -1`, is reported by clap with the accepted forms of the value
//! and ends the process with a non-zero exit code, before the terminal is
//! touched.

use clap::Parser;
use sigilfall_core::rng::Seed;

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

    /// Draw with plain ASCII characters instead of Unicode glyphs, for fonts
    /// that lack them or terminals that draw them on two cells. Applies to
    /// this session only, whatever the glyph option says.
    #[arg(long)]
    pub ascii: bool,
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
    fn no_argument_gives_no_seed_and_no_ascii() {
        let cli = parse(&[]).unwrap();

        assert_eq!(cli.seed, None);
        assert!(!cli.ascii);
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
    }

    #[test]
    fn options_combine_in_any_order() {
        let expected = Cli {
            seed: Some(Seed::new(7)),
            ascii: true,
        };

        assert_eq!(parse(&["--ascii", "--seed", "7"]).unwrap(), expected);
        assert_eq!(parse(&["--seed", "7", "--ascii"]).unwrap(), expected);
    }

    #[test]
    fn unknown_options_are_refused() {
        let error = parse(&["--graine", "42"]).unwrap_err();

        assert_eq!(error.kind(), ErrorKind::UnknownArgument);
        assert_ne!(error.exit_code(), 0);
    }
}
