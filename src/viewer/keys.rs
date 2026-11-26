#[cfg(target_os = "macos")]
const COPY: &str = "Cmd-c";

#[cfg(not(target_os = "macos"))]
const COPY: &str = "Super-c";

pub struct Row {
    pub keys: &'static [&'static str],
    pub what: &'static str,
}

pub struct Section {
    pub name: &'static str,
    pub rows: &'static [Row],
}

const fn row(keys: &'static [&'static str], what: &'static str) -> Row {
    Row { keys, what }
}

pub const SECTIONS: &[Section] = &[
    Section {
        name: "Scroll",
        rows: &[
            row(&["↓", "j", "Ctrl-e"], "Down a line"),
            row(&["↑", "k", "Ctrl-y"], "Up a line"),
            row(&["d", "Ctrl-d"], "Down half a screen"),
            row(&["u", "Ctrl-u"], "Up half a screen"),
            row(&["Space", "PgDn"], "Down a screen"),
            row(&["PgUp", "Ctrl-b"], "Up a screen"),
            row(&["gg", "Home"], "To the top"),
            row(&["G", "End"], "To the bottom"),
            row(&["←", "h"], "Wide code and tables left"),
            row(&["→", "l"], "Wide code and tables right"),
            row(&["Backspace", "Ctrl-o"], "Back to before a jump or the last file"),
        ],
    },
    Section {
        name: "Mouse",
        rows: &[
            row(&["wheel"], "Scroll 3 lines"),
            row(&["Shift-wheel", "drag its bar"], "Scroll a code block or table sideways"),
            row(&["click a link"], "Open it, or go to a # link's heading"),
            row(&["click a button"], "Copy code, toggle details, zoom diagrams"),
            row(&["drag"], "Select text"),
            row(&["double-click"], "Select a word"),
            row(&[COPY], "Copy the selection"),
            row(&["click a hint"], "Run the key it names in the bottom row"),
        ],
    },
    Section {
        name: "Search",
        rows: &[
            row(&["/", "Ctrl-f"], "Search the page"),
            row(&["n", "Enter"], "Next match"),
            row(&["N"], "Previous match"),
            row(&["Esc"], "Clear the search or close the contents"),
            row(&["Ctrl-u"], "In the prompt, clear it"),
            row(&["Ctrl-w"], "In the prompt, delete a word"),
        ],
    },
    Section {
        name: "Contents",
        rows: &[
            row(&["t", "Ctrl-t"], "Show or hide the table of contents"),
            row(&["1-9"], "Jump to section N, again for subsections"),
            row(&["0"], "Make the next digit step backwards"),
            row(&["J", "]]"], "Next section"),
            row(&["K", "[["], "Previous section"),
            row(&["D"], "Scroll the list down"),
            row(&["U"], "Scroll the list up"),
            row(&["wheel", "drag its bar"], "Scroll the list"),
            row(&["click an entry"], "Jump to that section"),
        ],
    },
    Section {
        name: "Other",
        rows: &[
            row(&["Ctrl-p"], "Fuzzy file picker for Markdowns in the current repo or folder"),
            row(&["e"], "Edit in $EDITOR (vi if unset) and reload"),
            row(&["c"], "Copy the entire Markdown source"),
            row(&["f"], "Activate this then type a label to 'click' on a link or button on the page without using your mouse"),
            row(&["w"], "Wrap long code lines"),
            row(&["T"], "Theme picker"),
            row(&["Ctrl-z"], "Pause from anywhere, fg to resume"),
            row(&["q", "Ctrl-c"], "Quit"),
        ],
    },
];

#[cfg(test)]
mod tests {
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn linux_names_the_super_key_for_copy() {
        let rows = || super::SECTIONS.iter().flat_map(|s| s.rows);
        assert!(rows().any(|r| r.keys == ["Super-c"]) && !rows().any(|r| r.keys.contains(&"Cmd-c")));
    }

    #[test]
    fn the_wheel_line_states_the_wheel_step() {
        let wheel = super::SECTIONS.iter().flat_map(|s| s.rows).find(|r| r.keys == ["wheel"]).unwrap();
        assert!(wheel.what.contains(&format!("Scroll {} lines", super::super::app::WHEEL)));
    }
}
