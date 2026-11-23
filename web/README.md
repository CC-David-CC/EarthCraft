# EarthCraft in WebOS

Build with Rust 1.99.1, the wasm32-unknown-unknown target, Trunk 0.21.14 and
Python 3. Run `trunk build --release --locked --dist dist` from the repository
root. Package the output in `native/` beside the WebOS mount and its stylesheet.

The browser imagery uses NASA GIBS Blue Marble from NASA Earth Observatory
and MODIS. The browser keeps imagery in memory and the HTTP cache. The
provider advertises imagery through zoom level 8. Coordinate navigation
and KML places use the EarthCraft core and the existing renderer. Desktop
imagery providers and other live services require their native environment.

Open and save KML or KMZ through WebOS Files. Save prompts for a destination
when the places have no Files binding. The browser checks the source revision
before saving over a bound places file. Save As keeps a separate copy after a
conflict. The corresponding source archive contains the component, boot
loader, mount and locked browser dependencies.
