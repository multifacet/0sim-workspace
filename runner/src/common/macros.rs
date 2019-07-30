//! Useful macros.

/// Time the given operations and push the time to the given `Vec<(String, Duration)>`.
macro_rules! time {
    ($timers:ident, $label:literal, $expr:expr) => {{
        let start = std::time::Instant::now();
        let result = $expr;
        let duration = std::time::Instant::now() - start;
        $timers.push(($label, duration));
        result
    }};
}

/// Given an ordered list of path components, combine them into a path string.
macro_rules! dir {
    ($first:expr $(, $part:expr)* $(,)?) => {{
        #[allow(unused_mut)]
        let mut path = String::from($first);

        $(
            path.push('/');
            path.extend(String::from($part).chars());
        )*

        path
    }}
}

/// Run a bunch of commands with the same shell and optionally the same CWD.
macro_rules! with_shell {
    ($shell:ident $(in $cwd:expr)? => $($cmd:expr),+ $(,)?) => {{
        let cmds = vec![$($cmd),+];

        $(
            let cmds: Vec<_> = cmds.into_iter().map(|cmd| cmd.cwd($cwd)).collect();
        )?

        for cmd in cmds.into_iter() {
            $shell.run(cmd)?;
        }
    }}
}
