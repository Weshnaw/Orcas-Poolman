# Orca's Poolman
This project aims to be a background service that will attempt to be a Filament configuration syncing utility using poolman as a backend


# Todo:
## Major
- Spoolman REST requests
- Reconciling the configurations and outputing the diffs
- Add better clap and env property tooling
- Refactor and Cleanup code
- add a install script for windows and linux
## Minor
- add an inheriting system to keep varieties of filaments that would use basically the same details in sync
  - allow local changes to sync up the chain
  - allow overwriting properties that then will not sync up the chain
  - have some properties that are defaulted to not sync up the chain (such as name, colour, etc)
- Detect available spools and add / remove filament configs from local
- Sync printers (though probably shouldn't / can't use spoolman for this)
- add some form of templating engine for things like custom filament profile names styles
- add some form to allow users to specify tag relations and their precedence in syncing