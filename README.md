# Orca's Poolman
This project aims to be a background service that will attempt to be a Filament configuration syncing utility using poolman as a backend


# Todo:
## Major
- Spoolman REST requests
- Reconciling the configurations
- Add better clap and env property tooling
- Refactor and Cleanup code
- add a install script for windows and linux
## Minor
- Detect available spools and add / remove filament configs from local
- Sync printers (though probably shouldn't / can't use spoolman for this)
- add some form of templating engine for things like custom filament profile names styles