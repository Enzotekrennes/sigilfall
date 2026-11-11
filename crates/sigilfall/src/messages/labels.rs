//! The fixed texts of every screen, in each language: titles, labels, menu
//! entries, key names and the lines chosen from the seed.
//!
//! Exposes [`Labels`], one field per text, and its two values, [`ENGLISH`]
//! and [`FRENCH`]. A text missing in one language does not compile, since
//! both values must fill every field. The texts that take values (a depth,
//! a name, a path) are functions of the `messages` module instead.
//!
//! French follows French typography: a no-break space (`\u{a0}`) before
//! `:`, `;`, `!` and `?`, so the mark never starts a line once wrapped
//! (`messages` tests check that every French text does).

/// Every fixed text the player reads, in one language. See [`ENGLISH`] for
/// what each one says.
#[derive(Debug)]
pub struct Labels {
    /// Reason given for a save that is not UTF-8 text.
    pub save_not_text: &'static str,
    /// Title of the side panel's health block.
    pub hp_label: &'static str,
    /// Label of the player's essence, in the side panel and the overlay.
    pub essence_label: &'static str,
    /// Title of the side panel's list of known sigils.
    pub sigils_title: &'static str,
    /// What that list says while the grimoire is empty.
    pub no_sigil_yet: &'static str,
    /// Title of the side panel's list of what the player sees.
    pub in_view_title: &'static str,
    /// Name of the player in that list, after the `@`.
    pub player_in_view: &'static str,
    /// Name of an engraved glyph not read yet, in that list.
    pub unread_glyph_in_view: &'static str,
    /// Name of an engraved glyph already read, in that list.
    pub read_glyph_in_view: &'static str,
    /// Name of the stairs down in that list.
    pub stairs_in_view: &'static str,
    /// Name of the seal of the bottom floor in that list.
    pub seal_in_view: &'static str,
    /// Title of the separator above the message log.
    pub log_title: &'static str,
    /// Title of the sigil overlay while the player traces.
    pub trace_title: &'static str,
    /// Title of the overlay once an unstable cast has been made.
    pub unstable_title: &'static str,
    /// Title of the overlay once a trace has fizzled.
    pub failed_title: &'static str,
    /// Title of the grimoire, in the overlay.
    pub grimoire_title: &'static str,
    /// The lines the grimoire shows while it is empty; `{glyph}` stands
    /// for the map glyph of an unread engraving. Each fits the 36 columns
    /// right of the grid.
    pub empty_grimoire: [&'static str; 4],
    /// Label before the nodes linked so far, in the overlay.
    pub trace_label: &'static str,
    /// Label before the time the trace takes, in the overlay.
    pub time_label: &'static str,
    /// Label before the pattern a near miss was meant to be.
    pub expected_label: &'static str,
    /// Label before the nodes the player linked, on a verdict.
    pub traced_label: &'static str,
    /// Verdict word of an unstable cast, in capitals so it reads without
    /// colour.
    pub unstable_word: &'static str,
    /// Verdict word of a fizzled cast, in capitals.
    pub failed_word: &'static str,
    /// How far an unstable trace was from its sigil: always one edit.
    pub one_node_off: &'static str,
    /// Why a fizzled trace near a known sigil cast nothing.
    pub no_known_pattern: &'static str,
    /// Why a fizzled trace whose pattern is not in the grimoire cast
    /// nothing.
    pub not_in_grimoire: &'static str,
    /// The lines under such a verdict: how sigils are learnt; `{glyph}` as
    /// in `empty_grimoire`. Each fits the 36 columns right of the grid.
    pub learn_from_glyphs: [&'static str; 3],
    /// Last line of the verdict of an unstable cast.
    pub unstable_outcome: &'static str,
    /// Last line of the verdict of a fizzled cast.
    pub failed_outcome: &'static str,
    /// Notice of the overlay when an empty trace is validated.
    pub empty_trace: &'static str,
    /// Title of the death summary, letter-spaced.
    pub death_title: &'static str,
    /// Lines under the death summary's title, one chosen from the seed; at
    /// most 60 characters each. The same seed picks the same position in
    /// every language.
    pub epitaphs: [&'static str; 8],
    /// Title of the victory screen, letter-spaced like `death_title`.
    pub victory_title: &'static str,
    /// Lines under the victory screen's title, chosen like `epitaphs`.
    pub victory_lines: [&'static str; 8],
    /// Label inside the frame around the seed on the end screens.
    pub seed_label: &'static str,
    /// Label of the deepest floor reached, in the end screens' counts.
    pub depth_reached_label: &'static str,
    /// Label of the number of turns played.
    pub turns_label: &'static str,
    /// Label of the number of creatures killed.
    pub kills_label: &'static str,
    /// Label of the number of glyphs read that taught a sigil.
    pub glyphs_label: &'static str,
    /// Label of the number of sigils cast.
    pub casts_label: &'static str,
    /// Label of the number of items used.
    pub items_used_label: &'static str,
    /// Name of the Enter key in the key help.
    pub enter_key: &'static str,
    /// Name of the Escape key in the key help.
    pub escape_key: &'static str,
    /// Name of the Backspace key in the key help.
    pub backspace_key: &'static str,
    /// The keys that use the chosen item in the inventory: Enter or `u`.
    pub use_keys: &'static str,
    /// What the quit key does on the title and end screens.
    pub quit_label: &'static str,
    /// What the quit key does on the game screen: the game is saved, then
    /// the program ends. Both are said, so a player never fears losing the
    /// run by quitting; short enough for the game's key help to fit 80
    /// columns (in French, with two columns between pairs).
    pub save_and_quit_label: &'static str,
    /// What Enter does on the end screens.
    pub title_label: &'static str,
    /// What the movement keys do, in the game's key help.
    pub move_label: &'static str,
    /// What the wait key does.
    pub wait_label: &'static str,
    /// What the key that opens the sigil overlay does.
    pub trace_key_label: &'static str,
    /// What the digits do in the overlay.
    pub link_label: &'static str,
    /// What Backspace does in the overlay.
    pub undo_label: &'static str,
    /// What Enter does while tracing.
    pub cast_label: &'static str,
    /// What Enter does on a verdict.
    pub continue_label: &'static str,
    /// What Escape does while tracing.
    pub cancel_label: &'static str,
    /// What Escape does while a direction is asked.
    pub back_label: &'static str,
    /// What the direction keys do while a direction is asked.
    pub aim_label: &'static str,
    /// What the pick-up key does.
    pub pick_up_label: &'static str,
    /// What the inventory key does.
    pub inventory_label: &'static str,
    /// What the descend key does.
    pub descend_label: &'static str,
    /// What the descend key does on the bottom floor, where it breaks the
    /// seal.
    pub unseal_label: &'static str,
    /// What the explore key does, in the game's key help when the row has
    /// room for it.
    pub explore_label: &'static str,
    /// Title of the inventory's frame.
    pub inventory_title: &'static str,
    /// What the arrows and the digits do in the lists.
    pub choose_label: &'static str,
    /// What the use keys do in the inventory.
    pub use_label: &'static str,
    /// What the drop key does in the inventory.
    pub drop_label: &'static str,
    /// What Escape does in the inventory.
    pub close_label: &'static str,
    /// Label of the depth on the status line.
    pub depth_label: &'static str,
    /// Label of the turn on the status line.
    pub turn_label: &'static str,
    /// Label of the seed at the right end of the status line.
    pub status_seed_label: &'static str,
    /// Label of the date at the right end of the status line of a daily
    /// run, in place of the seed.
    pub status_daily_label: &'static str,
    /// Line under the game's name on the title screen.
    pub tagline: &'static str,
    /// Menu entry that resumes the saved game.
    pub continue_item: &'static str,
    /// Menu entry that starts a new game on a random seed.
    pub new_game_item: &'static str,
    /// Menu entry that starts a new game on a typed seed.
    pub seed_item: &'static str,
    /// Menu entry that starts the daily dungeon of today's UTC date.
    pub daily_item: &'static str,
    /// Menu entry that opens the options screen.
    pub options_item: &'static str,
    /// Menu entry that leaves the game.
    pub quit_item: &'static str,
    /// What Enter does in the menus.
    pub confirm_label: &'static str,
    /// What the title screen asks while a seed is typed.
    pub seed_prompt: &'static str,
    /// Label before the field where the seed is typed.
    pub seed_field_label: &'static str,
    /// The forms a seed is accepted in, under the field.
    pub seed_forms: &'static str,
    /// What the seed entry says when Enter is pressed on an empty field.
    pub empty_seed: &'static str,
    /// What Enter does in the seed entry.
    pub start_label: &'static str,
    /// What Backspace does in the seed entry.
    pub erase_label: &'static str,
    /// What Escape does on the seed entry and the options screen.
    pub menu_back_label: &'static str,
    /// Question of the confirmation before a new game replaces a saved
    /// one.
    pub abandon_question: &'static str,
    /// Answer that keeps the saved game, chosen at first.
    pub keep_item: &'static str,
    /// Answer that deletes the saved game and starts the new one.
    pub abandon_item: &'static str,
    /// What Escape does on the confirmation.
    pub keep_label: &'static str,
    /// Title of the options screen, letter-spaced.
    pub options_title: &'static str,
    /// Label of the glyph option.
    pub glyphs_option: &'static str,
    /// Label of the colour option.
    pub colors_option: &'static str,
    /// Label of the theme option.
    pub theme_option: &'static str,
    /// Label of the language option.
    pub language_option: &'static str,
    /// Label of the keyboard option, whose values are the names of the
    /// layouts, the same in every language (`Keyboard::name`).
    pub keyboard_option: &'static str,
    /// Last entry of the options screen, back to the title screen.
    pub back_item: &'static str,
    /// Value of the glyph option for Unicode glyphs.
    pub unicode_value: &'static str,
    /// Value of the glyph option for ASCII glyphs.
    pub ascii_value: &'static str,
    /// Value of the colour option for the 16 ANSI colours.
    pub ansi16_value: &'static str,
    /// Value of the colour option for no colour.
    pub monochrome_value: &'static str,
    /// Value of the theme option for the dark theme.
    pub dark_value: &'static str,
    /// Value of the theme option for the light theme.
    pub light_value: &'static str,
    /// What each entry of the options screen does: glyphs, colours, theme,
    /// language, keyboard, back. Each fits 80 columns.
    pub option_help: [&'static str; 6],
    /// Label of the sample line of the options screen.
    pub sample_label: &'static str,
    /// Note of the options screen while `--ascii` is given.
    pub ascii_forced: &'static str,
    /// Note of the options screen when the light theme falls back to
    /// monochrome on a 16-colour terminal.
    pub light_in_monochrome: &'static str,
    /// What changes an option, in the key help of the options screen.
    pub change_label: &'static str,
}

impl Labels {
    /// Returns every text of these labels, the lines of the arrays one by one,
    /// so tests can check them all; the destructuring makes a new field a
    /// compile error here until it is listed.
    #[cfg(test)]
    pub fn all(&self) -> Vec<&'static str> {
        let Self {
            save_not_text,
            hp_label,
            essence_label,
            sigils_title,
            no_sigil_yet,
            in_view_title,
            player_in_view,
            unread_glyph_in_view,
            read_glyph_in_view,
            stairs_in_view,
            seal_in_view,
            log_title,
            trace_title,
            unstable_title,
            failed_title,
            grimoire_title,
            empty_grimoire,
            trace_label,
            time_label,
            expected_label,
            traced_label,
            unstable_word,
            failed_word,
            one_node_off,
            no_known_pattern,
            not_in_grimoire,
            learn_from_glyphs,
            unstable_outcome,
            failed_outcome,
            empty_trace,
            death_title,
            epitaphs,
            victory_title,
            victory_lines,
            seed_label,
            depth_reached_label,
            turns_label,
            kills_label,
            glyphs_label,
            casts_label,
            items_used_label,
            enter_key,
            escape_key,
            backspace_key,
            use_keys,
            quit_label,
            save_and_quit_label,
            title_label,
            move_label,
            wait_label,
            trace_key_label,
            link_label,
            undo_label,
            cast_label,
            continue_label,
            cancel_label,
            back_label,
            aim_label,
            pick_up_label,
            inventory_label,
            descend_label,
            unseal_label,
            explore_label,
            inventory_title,
            choose_label,
            use_label,
            drop_label,
            close_label,
            depth_label,
            turn_label,
            status_seed_label,
            status_daily_label,
            tagline,
            continue_item,
            new_game_item,
            seed_item,
            daily_item,
            options_item,
            quit_item,
            confirm_label,
            seed_prompt,
            seed_field_label,
            seed_forms,
            empty_seed,
            start_label,
            erase_label,
            menu_back_label,
            abandon_question,
            keep_item,
            abandon_item,
            keep_label,
            options_title,
            glyphs_option,
            colors_option,
            theme_option,
            language_option,
            keyboard_option,
            back_item,
            unicode_value,
            ascii_value,
            ansi16_value,
            monochrome_value,
            dark_value,
            light_value,
            option_help,
            sample_label,
            ascii_forced,
            light_in_monochrome,
            change_label,
        } = self;
        let mut texts = vec![
            *save_not_text,
            *hp_label,
            *essence_label,
            *sigils_title,
            *no_sigil_yet,
            *in_view_title,
            *player_in_view,
            *unread_glyph_in_view,
            *read_glyph_in_view,
            *stairs_in_view,
            *seal_in_view,
            *log_title,
            *trace_title,
            *unstable_title,
            *failed_title,
            *grimoire_title,
            *trace_label,
            *time_label,
            *expected_label,
            *traced_label,
            *unstable_word,
            *failed_word,
            *one_node_off,
            *no_known_pattern,
            *not_in_grimoire,
            *unstable_outcome,
            *failed_outcome,
            *empty_trace,
            *death_title,
            *victory_title,
            *seed_label,
            *depth_reached_label,
            *turns_label,
            *kills_label,
            *glyphs_label,
            *casts_label,
            *items_used_label,
            *enter_key,
            *escape_key,
            *backspace_key,
            *use_keys,
            *quit_label,
            *save_and_quit_label,
            *title_label,
            *move_label,
            *wait_label,
            *trace_key_label,
            *link_label,
            *undo_label,
            *cast_label,
            *continue_label,
            *cancel_label,
            *back_label,
            *aim_label,
            *pick_up_label,
            *inventory_label,
            *descend_label,
            *unseal_label,
            *explore_label,
            *inventory_title,
            *choose_label,
            *use_label,
            *drop_label,
            *close_label,
            *depth_label,
            *turn_label,
            *status_seed_label,
            *status_daily_label,
            *tagline,
            *continue_item,
            *new_game_item,
            *seed_item,
            *daily_item,
            *options_item,
            *quit_item,
            *confirm_label,
            *seed_prompt,
            *seed_field_label,
            *seed_forms,
            *empty_seed,
            *start_label,
            *erase_label,
            *menu_back_label,
            *abandon_question,
            *keep_item,
            *abandon_item,
            *keep_label,
            *options_title,
            *glyphs_option,
            *colors_option,
            *theme_option,
            *language_option,
            *keyboard_option,
            *back_item,
            *unicode_value,
            *ascii_value,
            *ansi16_value,
            *monochrome_value,
            *dark_value,
            *light_value,
            *sample_label,
            *ascii_forced,
            *light_in_monochrome,
            *change_label,
        ];
        texts.extend(*empty_grimoire);
        texts.extend(*learn_from_glyphs);
        texts.extend(*epitaphs);
        texts.extend(*victory_lines);
        texts.extend(*option_help);
        texts
    }
}

/// The texts in English.
pub const ENGLISH: Labels = Labels {
    save_not_text: "the save is not text",
    hp_label: "HP",
    essence_label: "Essence",
    sigils_title: "SIGILS",
    no_sigil_yet: "none yet",
    in_view_title: "IN VIEW",
    player_in_view: "you",
    unread_glyph_in_view: "carved glyph",
    read_glyph_in_view: "known glyph",
    stairs_in_view: "stairs down",
    seal_in_view: "seal",
    log_title: "Log",
    trace_title: "Trace a sigil",
    unstable_title: "Unstable trace",
    failed_title: "Failed trace",
    grimoire_title: "GRIMOIRE",
    empty_grimoire: [
        "No sigil known yet.",
        "Step on a glyph carved",
        "in the floor, {glyph} on the",
        "map, to learn its sigil.",
    ],
    trace_label: "Trace",
    time_label: "Time",
    expected_label: "expected",
    traced_label: "traced",
    unstable_word: "UNSTABLE",
    failed_word: "FAILED",
    one_node_off: "one node off",
    no_known_pattern: "no known pattern",
    not_in_grimoire: "not in your grimoire",
    learn_from_glyphs: [
        "Sigils are learnt from the glyphs",
        "carved in the floor, {glyph} on the",
        "map: step on one to read it.",
    ],
    unstable_outcome: "The magic lashes back.",
    failed_outcome: "The spell fizzles.",
    empty_trace: "Link at least one node first.",
    death_title: "T H E   D E S C E N T   E N D S",
    epitaphs: [
        "The walls will keep your trace.",
        "Your last stroke is still drying on the stone.",
        "The ink outlived the hand that traced it.",
        "Somewhere below, a glyph waits for another reader.",
        "The dark learnt your sigils by heart.",
        "Your essence seeps into the old carvings.",
        "The grimoire closes; its margins remember you.",
        "Dust settles over an unfinished pattern.",
    ],
    victory_title: "T H E   S E A L   B R E A K S",
    victory_lines: [
        "Your trace was true. The dungeon heard it.",
        "The last sigil was the one that opened the way.",
        "The wax runs, and the dark lets go of you.",
        "Ten floors of stone remember a steady hand.",
        "The carvings fall silent behind you.",
        "The grimoire closes on a finished page.",
        "What the seal held is yours to read now.",
        "Somewhere above, the first glyph still glows.",
    ],
    seed_label: "SEED",
    depth_reached_label: "Depth reached",
    turns_label: "Turns",
    kills_label: "Creatures slain",
    glyphs_label: "Glyphs learnt",
    casts_label: "Sigils cast",
    items_used_label: "Items used",
    enter_key: "Enter",
    escape_key: "Esc",
    backspace_key: "Backspace",
    use_keys: "Enter/u",
    quit_label: "quit",
    save_and_quit_label: "save & quit",
    title_label: "title screen",
    move_label: "move",
    wait_label: "wait",
    trace_key_label: "trace",
    link_label: "link a node",
    undo_label: "undo",
    cast_label: "cast",
    continue_label: "continue",
    cancel_label: "cancel",
    back_label: "back to the trace",
    aim_label: "aim",
    pick_up_label: "take",
    inventory_label: "pack",
    descend_label: "descend",
    unseal_label: "unseal",
    explore_label: "explore",
    inventory_title: "Inventory",
    choose_label: "choose",
    use_label: "use",
    drop_label: "drop",
    close_label: "close",
    depth_label: "Depth",
    turn_label: "Turn",
    status_seed_label: "Seed",
    status_daily_label: "Daily",
    tagline: "Trace true. The dungeon listens.",
    continue_item: "Continue",
    new_game_item: "New game",
    seed_item: "Seed",
    daily_item: "Daily",
    options_item: "Options",
    quit_item: "Quit",
    confirm_label: "confirm",
    seed_prompt: "Type the seed of the dungeon, then press Enter.",
    seed_field_label: "Seed",
    seed_forms: "A number such as 42, or the shared form 7F3A-91C2-0B5E-D4A8.",
    empty_seed: "Type a seed first.",
    start_label: "start",
    erase_label: "erase",
    menu_back_label: "back",
    abandon_question: "Start a new game?",
    keep_item: "Keep the game in progress",
    abandon_item: "Abandon it and start anew",
    keep_label: "keep it",
    options_title: "O P T I O N S",
    glyphs_option: "Glyphs",
    colors_option: "Colours",
    theme_option: "Theme",
    language_option: "Language",
    keyboard_option: "Keyboard",
    back_item: "Back",
    unicode_value: "Unicode",
    ascii_value: "ASCII",
    ansi16_value: "16 colours",
    monochrome_value: "Monochrome",
    dark_value: "Dark",
    light_value: "Light",
    option_help: [
        "Unicode draws stone walls and sigils; ASCII suits any font.",
        "Auto follows the terminal; 16 colours or Monochrome suit plain ones.",
        "Dark ink or light parchment. Light needs true colour.",
        "Every text of the game, in English or in French.",
        "QWERTY moves with WASD; AZERTY with ZQSD, and the top row needs no Shift.",
        "Options are saved as soon as they change.",
    ],
    sample_label: "Sample",
    ascii_forced: "--ascii is on: ASCII glyphs for this session.",
    light_in_monochrome: "Light needs true colour: drawn in monochrome here.",
    change_label: "change",
};

/// The texts in French.
pub const FRENCH: Labels = Labels {
    save_not_text: "la sauvegarde n'est pas un texte",
    hp_label: "PV",
    essence_label: "Essence",
    sigils_title: "SIGILLES",
    no_sigil_yet: "aucun encore",
    in_view_title: "EN VUE",
    player_in_view: "vous",
    unread_glyph_in_view: "glyphe gravé",
    read_glyph_in_view: "glyphe connu",
    stairs_in_view: "escalier",
    seal_in_view: "sceau",
    log_title: "Journal",
    trace_title: "Tracer un sigille",
    unstable_title: "Tracé instable",
    failed_title: "Tracé raté",
    grimoire_title: "GRIMOIRE",
    empty_grimoire: [
        "Aucun sigille connu.",
        "Marchez sur un glyphe gravé",
        "dans le sol, {glyph} sur la carte,",
        "pour apprendre son sigille.",
    ],
    trace_label: "Tracé",
    time_label: "Durée",
    expected_label: "attendu",
    traced_label: "tracé",
    unstable_word: "INSTABLE",
    failed_word: "RATÉ",
    one_node_off: "à un nœud près",
    no_known_pattern: "aucun motif connu",
    not_in_grimoire: "absent de votre grimoire",
    learn_from_glyphs: [
        "Les sigilles s'apprennent en lisant",
        "les glyphes gravés au sol, {glyph} sur",
        "la carte\u{a0}: marchez sur l'un d'eux.",
    ],
    unstable_outcome: "La magie se retourne contre vous.",
    failed_outcome: "Le sort se dissipe.",
    empty_trace: "Reliez d'abord au moins un nœud.",
    death_title: "L A   D E S C E N T E   S ' A C H È V E",
    epitaphs: [
        "Les murs garderont votre tracé.",
        "Votre dernier trait sèche encore sur la pierre.",
        "L'encre a survécu à la main qui l'a tracée.",
        "Plus bas, un glyphe attend un autre lecteur.",
        "Les ténèbres ont appris vos sigilles par cœur.",
        "Votre essence s'infiltre dans les vieilles gravures.",
        "Le grimoire se referme\u{a0}; ses marges se souviennent.",
        "La poussière recouvre un motif inachevé.",
    ],
    victory_title: "L E   S C E A U   S E   B R I S E",
    victory_lines: [
        "Votre tracé était juste. Le donjon l'a entendu.",
        "Le dernier sigille était celui qui ouvrait la voie.",
        "La cire coule, et les ténèbres vous laissent partir.",
        "Dix étages de pierre se souviennent d'une main sûre.",
        "Les gravures se taisent derrière vous.",
        "Le grimoire se referme sur une page achevée.",
        "Ce que gardait le sceau, à vous de le lire.",
        "Là-haut, le premier glyphe brille encore.",
    ],
    seed_label: "GRAINE",
    depth_reached_label: "Profondeur atteinte",
    turns_label: "Tours",
    kills_label: "Créatures tuées",
    glyphs_label: "Glyphes appris",
    casts_label: "Sigilles tracés",
    items_used_label: "Objets utilisés",
    enter_key: "Entrée",
    escape_key: "Échap",
    backspace_key: "Retour arrière",
    use_keys: "Entrée/u",
    quit_label: "quitter",
    save_and_quit_label: "sauver et quitter",
    title_label: "écran titre",
    move_label: "aller",
    wait_label: "repos",
    trace_key_label: "tracer",
    link_label: "relier un nœud",
    undo_label: "effacer",
    cast_label: "lancer",
    continue_label: "continuer",
    cancel_label: "annuler",
    back_label: "retour au tracé",
    aim_label: "viser",
    pick_up_label: "saisir",
    inventory_label: "sac",
    descend_label: "descente",
    unseal_label: "briser",
    explore_label: "explorer",
    inventory_title: "Inventaire",
    choose_label: "choisir",
    use_label: "utiliser",
    drop_label: "poser",
    close_label: "fermer",
    depth_label: "Profondeur",
    turn_label: "Tour",
    status_seed_label: "Graine",
    status_daily_label: "Donjon du jour",
    tagline: "Tracez juste. Le donjon écoute.",
    continue_item: "Continuer",
    new_game_item: "Nouvelle partie",
    seed_item: "Graine",
    daily_item: "Donjon du jour",
    options_item: "Options",
    quit_item: "Quitter",
    confirm_label: "valider",
    seed_prompt: "Tapez la graine du donjon, puis appuyez sur Entrée.",
    seed_field_label: "Graine",
    seed_forms: "Un nombre comme 42, ou la forme partagée 7F3A-91C2-0B5E-D4A8.",
    empty_seed: "Tapez d'abord une graine.",
    start_label: "commencer",
    erase_label: "effacer",
    menu_back_label: "retour",
    abandon_question: "Commencer une nouvelle partie\u{a0}?",
    keep_item: "Garder la partie en cours",
    abandon_item: "L'abandonner et recommencer",
    keep_label: "la garder",
    options_title: "O P T I O N S",
    glyphs_option: "Glyphes",
    colors_option: "Couleurs",
    theme_option: "Thème",
    language_option: "Langue",
    keyboard_option: "Clavier",
    back_item: "Retour",
    unicode_value: "Unicode",
    ascii_value: "ASCII",
    ansi16_value: "16 couleurs",
    monochrome_value: "Monochrome",
    dark_value: "Sombre",
    light_value: "Clair",
    option_help: [
        "Unicode dessine pierre et sigilles\u{a0}; ASCII convient à toute police.",
        "Auto suit le terminal\u{a0}; 16 couleurs ou Monochrome pour les plus simples.",
        "Encre sombre ou parchemin clair. Clair demande les vraies couleurs.",
        "Tous les textes du jeu, en anglais ou en français.",
        "QWERTY\u{a0}: WASD pour bouger\u{a0}; AZERTY\u{a0}: ZQSD, et la rangée du haut sans Maj.",
        "Les options sont enregistrées dès qu'elles changent.",
    ],
    sample_label: "Aperçu",
    ascii_forced: "--ascii est actif\u{a0}: glyphes ASCII pour cette session.",
    light_in_monochrome: "Clair demande les vraies couleurs\u{a0}: dessiné en monochrome ici.",
    change_label: "changer",
};
