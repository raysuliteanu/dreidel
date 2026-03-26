# TODOs

- have a help dialog that includes the current keymap but also some info about
  the tool specifically the version from the Cargo.toml manifest as well as the jj
  change id and a link to the the github website also from the manifest or maybe
  the crate on crates.io

- ensure `tokio` minimal feature set; don't use 'full', only what's required
- update Cargo.toml's [package] section with fields like description, authors,
  license, repository, homepage, keywords, categories, readme, and edition to make
  it ready for cargo publish.

- ensure that the current focused component is visually highlighted e.g. border
  is blue vs others just white (whatever the foreground color is); similarly it
  would be nice to have the available keyboard shortcuts shown and the current
  selected one highlighted; this is similar to how bpytop does it and helps make
  the user know what the keyboard shortcuts are without having to memorize them
- tab should cycle through the components and maybe left arrow/right arrow
  between the available key actions within a component

- the --init-config option should just print the config to stdout and then exit
