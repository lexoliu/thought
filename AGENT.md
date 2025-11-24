# Agent Notes

- Do not consume `thought-plugin` via local `path` or `[patch]`. Always depend on it with `git = "https://github.com/lexoliu/thought.git"`.
- Do not add `.cargo/config` patches that override `thought-plugin` to a local path. Publish required changes to the git repo instead.
