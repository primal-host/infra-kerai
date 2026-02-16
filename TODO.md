# Kerai — TODO

## Future Investigation

- **rustfmt vs prettyplease for reconstruction**: Currently using prettyplease (library, no rustc dependency) but it strips regular comments. rustfmt preserves all comments since it operates at the token stream level. Comments from developers (human or AI) carry informational value worth preserving. Investigate switching reconstruction to shell out to rustfmt, assembling stored comment nodes into raw source before formatting. Tradeoff: binary dependency vs comment fidelity.
