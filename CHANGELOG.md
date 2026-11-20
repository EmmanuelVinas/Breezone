# Changelog

All notable changes are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.12.0-beta.1] - 2026-11-20

### Added

- New integrations, all local except the forecast:
  - **ESPHome**, by its native API (port 6053, encrypted with the device's API key, or in clear):
    a connection kept open, each change pushed by the device; switches, lights (brightness, white,
    colours), covers, power sensors and the air sensors (temperature, humidity, pressure, CO₂,
    illuminance).
  - **Tasmota**, by its HTTP API: relays, lights, energy metering, air sensors.
  - **TP-Link Kasa and Tapo** plugs, power strips and bulbs: the old Kasa protocol (port 9999)
    or KLAP with the TP-Link account (it only opens the local session); power and energy of the day.
  - **Reolink** cameras, doorbells and NVR: motion and what caused it (person, vehicle, animal),
    the picture, and a doorbell's ring as a button press for the automations.
  - **Forecast.Solar**: the solar production expected now, today, still to come and tomorrow.
- The search on the network finds ESPHome and Tasmota modules, Kasa plugs and Reolink cameras.
- Settings › Modules: a module whose driver asks more than its address (credentials, a key, its
  panels) is added with a form, and a « Settings » card changes them later; passwords and keys stay
  encrypted on the server and are never shown again.
- New traits `environment.sensor@1` and `solar.forecast@1`; new device types « Sensors » and
  « Cameras ».
- Mobile app, home: the cards of the web overview that it lacked: a light or the lights of a room
  (brightness, white, colours), the price of the kWh with the Tempo colours of today and tomorrow,
  a measure with its curve of the last 24 hours (the setting of a device card), and the chart of
  the ventilated rooms (one measure, one curve per room).
- The licences of the software Breezone includes: those of the server's crates and of the web UI's
  packages in `third-party-licenses.txt`, linked at the bottom of the menu; those of the app's
  libraries in its Settings › Licences. Breezone's own licence is in the Docker image.
- Weather: « Weather data: Open-Meteo.com » under the forecasts (web and app), as Open-Meteo asks.
- Energy page of the app: Day, 7 days, 30 days, Year and 5 years as on the web; the day's curve goes
  to the previous or next day, or any day chosen in a calendar; the other periods in bars.
- Mobile app, home: the wave turns in the middle while the home loads, instead of « Installation
  not found ».

### Changed

- Mobile app: the demonstration has the modules too: lights and their scenes, shutters, the solar
  production, the grid, a water heater and a charger on the surplus, ventilation room by room, a
  remote, the weather and a few automations; their measures follow the time of day.
- Showcase site and README: Breezone is presented as a simple home automation, its source code
  available (« au code ouvert », not « open source », as its licence is), with the list of its
  integrations; new screenshots of the app, taken in its demonstration.
- Settings › Accounts and services: a grid of small tiles (state, a line, the switch), four by row
  on a wide screen; the steps and the credentials of an account open in a dialog on a click.
- Settings: the tabs in a new order (Breezone first), « Devices » renamed « Connected devices ».

## [0.11.0] - 2026-11-19

The first stable release of 0.11: everything of its betas below (linked accounts and services,
Netatmo · Legrand, EZVIZ and the cameras page, the configurable overview and mobile home, the
solar surplus for the air conditioning…), and what follows.

### Added

- Overview: a small block « Price and Tempo colour » (a quarter of a row): the price of the kWh
  now, its slot, and the colour of today and of tomorrow.
- Overview: a block « Light or room, brightness and colours », as the detail of the lights page:
  its big brightness bar, the white temperature and the colours (each can be left out), for one
  light or for all the lights of a room at once.
- Overview: a chart of the ventilated rooms (section Ventilation): one measure (humidity, flow,
  CO₂, temperature, VOC, level) of every room, one curve each, over 24 hours or 7 days; a curve
  hides by its legend.
- Solar surplus steering: a zone of the air conditioning can follow the surplus, in the same order
  of priority as the charger and the relays. It heats to its setpoint when it is cold outside and
  its system heats, cools when it is hot and its system cools, and is put back as it was when the
  surplus runs short; a zone set by hand is left alone. Programmations show the steering as a card
  of its own (set on the energy page), and an automation that drives a device it runs too is
  flagged on both sides. The recipe « heat or cool on the surplus » waits for the car to be
  unplugged (the charger first).

### Fixed

- Overview › « Add a block »: the air quality of a zone is offered only for the zones whose sensor
  measures it; the others showed an empty preview (their card stays hidden without a measure).

## [0.11.0-beta.44] - 2026-11-19

### Added

- Overview: the « Device » block of a measure can show its curve of the last 24 hours, its value
  large, as the figures of the energy page. In « Add a block », each component has a fine frame,
  so that its name goes with its preview.
- Netatmo · Legrand: in the card « Matching with your devices », a device of its own (a line of
  the Ecometer…) is named there as one wishes; the name Netatmo gives it shows below when it differs.
- Energy › « Measured devices »: each measure tells the module it comes from (Shelly, Hoymiles,
  Legrand Écocompteur…), two measures may bear the same name.

### Fixed

- Overview: the block of a shutter, a light, a ventilated room or a zone is as high as the others
  of its row (the shutter's window in the middle, its buttons below).
- Automations and scenes: those of the modules alone (shutters, lights…) are this server's: they are
  no longer shown on an installation reached through another server, as if they were its own (one
  rule, one switch, on both). Web and mobile.
- The lists of choices open below their field and keep where they are scrolled: aligned on the
  chosen item, they jumped back to it at each refresh of the page (every few seconds).
- Settings › Modules › Airzone: the dot of the module only counts this server's installations (one
  reached through another server no longer turns it orange), and the card « Your installations »
  lists them again (this server's were all left out). It now shows only when there is something to
  link: an installation of the Airzone account not reached yet, or a WebServer in none of them.

## [0.11.0-beta.43] - 2026-11-18

### Fixed

- The mobile app builds again: the types generated for the button's new « press » action no
  longer declare `ButtonPress` twice (the enum of the press is `ButtonPressValue`).

## [0.11.0-beta.42] - 2026-11-18

### Added

- A remote (`button@1`, such as a Legrand remote switch through HomeKit) can be added to the
  overview with the « Device » block: its buttons are pressed from there, each starting the
  automations set on it, and each tells which ones. A press from Breezone does what a press on the
  remote does for Breezone; what the remote does in its own app (Legrand) is not asked.

### Fixed

- Netatmo · Legrand: the lines of the Ecometer (NLE) show their power. Netatmo gives none in their
  status: it is now the mean of their last whole half hour, from their measures (read every
  10 minutes; Legrand tells them late).

## [0.11.0-beta.41] - 2026-11-18

### Added

- EZVIZ account (Settings › Accounts and services), with the user name and password of the EZVIZ
  app (no developer key), two-step verification by code. Its module comes with it: the cameras
  and doorbells, online or not, their last event and its picture (decrypted with the verification
  code of the cameras), and a « Cameras » page. A ring of the doorbell is a button press: it shows
  in the activity and can start an automation.

### Fixed

- Automations: two rules whose conditions cannot both be true (outdoor below 14° and above 25°,
  « Salon · Mode : Heat » and « … : Cool »…) are no longer said to contradict each other; what one
  does otherwise still counts.

## [0.11.0-beta.40] - 2026-11-18

### Added

- Netatmo · Legrand: each device of the cloud is matched with its local device by the words of its
  name (« Volet roulant salon institut » is « Volet Institut »: the words that only name a kind,
  shutter, roller…, are left out), and can be set by hand in the module's card « Matching with your
  devices »: same as a local device, a device of its own, or ignored. The duplicates made before go.
- Any device of any module can be ignored (Settings › Modules, the hidden-eye button of its row):
  it leaves Breezone, and the card « Ignored devices » brings it back. Each line of the matching
  card shows its last reading (power, position, on/off) to tell the Ecometer's lines apart.
- Settings › « Accounts and services »: the weather (Open-Meteo, MeteoAlarm) is a linked service,
  each account or service has a switch to turn it off without forgetting it (the Tempo colours
  then come from the public relay, the Netatmo module stops…), and a search narrows the list.

## [0.11.0-beta.39] - 2026-11-18

### Added

- Netatmo · Legrand module (cloud), added with the Netatmo account: the lines of the Ecometer as
  energy measures, and what the gateway does not give HomeKit. A device the local modules already
  have (same name and kind) is not repeated: it stays driven locally and goes through the cloud
  only for what the local cannot do, or when the local does not answer. A Legrand shutter can now
  be stopped midway (the stop button shows), through the cloud.

## [0.11.0-beta.38] - 2026-11-18

### Added

- Automations: the recipe “Solar surplus: heat or cool the home” — rather than giving the surplus
  back to the grid, heat the rooms chosen when it is cold outside (or cool them when it is warm) up
  to the temperature wanted, and put each room back as it was once the home draws from the grid
  again. A form sets the rooms, the two temperatures, the outdoor thresholds and the surplus; one
  rule per Airzone system, each following the mode of its own system (never changing it).
- Linked accounts (Settings › Linked accounts, administrators): a connection to an online
  service, always on top of what Breezone reads locally, its secrets encrypted on the server. The
  Airzone Cloud account moves there without asking anything again. New ones:
  - **RTE**: the official colour of the Tempo days (the public relay stays when there is none, or
    when RTE fails);
  - **Linky** through Conso API: what Enedis measured, day by day and per half hour, read every
    6 hours, with its cost at the tariff (each half hour at its price), and how much the modules
    measure of it; a “Linky consumption” block for dashboards;
  - **Netatmo · Legrand Home + Control**: connected with one's own Netatmo app (client id, secret and
    a refresh token from its token generator, nothing to open to the internet); Breezone renews the
    access itself and shows the homes, rooms and modules it reaches.
- Dashboards: a “Self-consumption / autonomy” figure, the two side by side (“47 % / 13 %”), on its
  own or ticked in the figures of the period (not shown unless chosen).

## [0.11.0-beta.37] - 2026-11-18

### Added

- Airzone schedules can be switched off all at once (web and app, administrators), so that
  Breezone's automations and schedules do not compete with them: those that were on are switched
  off in Airzone, and only they come back on when switched on again. While they are all off, the
  section is folded; one switched on again, even in the Airzone app, unfolds it.
- App: the automations of Breezone come first in Schedules, the Airzone schedules after.
- Settings › Modules: a Philips Hue bridge and a HomeKit accessory (Legrand, VELUX…) can also be
  added by their address (the bridge's button, or the accessory's code, as when found).
- Dashboards: a “Grid cost / solar gain” figure, the two side by side (“1,19 € / 0,10 €”): what was
  bought from the grid, against what the solar saved or sold. Added on its own or ticked in the
  figures of the period (not shown unless chosen).
- App: the page shortcuts of Home can show some pages only (chosen in the card's settings), and a
  single page can be added as a half card of its own.


### Changed

- The network searched for modules defaults to the one of the hardware already added (WebServers,
  modules) when the server runs in a container, instead of asking for it.
## [0.11.0-beta.36] - 2026-11-18

### Added

- Dashboards: the figures of the period are shown in the order chosen — in the block's settings,
  each figure ticked moves up or down.
- Dashboards (web and app): an “Empty space” block, a cell left empty (¼, ½ or a whole row), to set
  a block apart on its own row without changing its size; seen only while arranging.

## [0.11.0-beta.35] - 2026-11-18

### Fixed

- HomeKit gateway unreachable after updates (“timed out, before the session was set up”): the
  search for lost sources no longer opens connections to the HomeKit accessories already added
  (they are found through mDNS only), so it no longer uses up the few connections of a gateway; a
  source is no longer taken for lost right after a restart (it is given 3 minutes to answer); and
  the server ends its HomeKit sessions cleanly before it stops or restarts on a new version.
- The web interface no longer names a brand of NAS: the image is updated “from your container
  manager”, and the hint for a reverse proxy without WebSocket gives the headers to add (Upgrade
  and Connection).

## [0.11.0-beta.34] - 2026-11-18

### Added

- Dashboards: “Add a block” shows every component of the home as it looks, by section on the left
  (overview, rooms, types, devices, charts, energy, climate, lights, charging, solar, ventilation)
  and their live previews on the right; a component is then set up beside its preview.
- Dashboards: the parts of the other pages as blocks — a zone's history and its air quality, the
  Hue scenes, the charger and its power, the solar production of the day and day by day, the
  micro-inverters and their panels, the house's ventilation. Each figure of the period
  (production, consumption, grid, self-consumption, autonomy, cost, solar gains, battery) can be
  added on its own.
- App: the same parts as cards of Home (zone history and air, charger and its power, solar
  production, micro-inverters, ventilation), and card settings: the devices of a room or a type
  the card shows (asked as soon as one is added), the scenes, the measures and period of a chart.

## [0.11.0-beta.33] - 2026-11-18

### Added

- App: a bottom bar with every page of the installation shown (Home, Climate, Energy, Solar,
  Charging, Lights, Shutters, Ventilation, Routines, Schedules, Settings); it scrolls sideways when
  they do not all fit. The installation chosen is kept from one launch to the next, and the list of
  installations opens from the title of Home.
- App: Home is made of cards each user arranges on the phone, apart from the web's overview (the
  server keeps it as the dashboard page `mobile`): page shortcuts, weather, presence, an energy
  figure, energy now, today and by use, climate zones, a room, a type of devices, a device, a chart,
  scenes. Arrange moves, resizes (half or full width), removes and adds cards; “Back to the
  suggested view” forgets them. The cards reuse the rows and cards of each page (a light and its
  brightness, a shutter and its buttons, a zone and its setpoint).

### Fixed

- A source that does not answer is no longer asked again every few seconds: once offline, it is
  read twice as late after each failure, up to every 5 minutes (“Test” still reads it at once), so
  that a struggling device — a HomeKit gateway after a restart of the server — can recover.
- HomeKit: the remotes are listened to only once the gateway answered a reading, so that a starting
  server no longer opens two sessions at once on a gateway that may still hold the previous ones.

## [0.11.0-beta.32] - 2026-11-17

### Added

- Automations: an action on a group — all the devices of a type in a room, or in the whole home
  (“Shutters · Living room”, “Shutters · whole home”) — offered at the top of each type in the list
  of devices. The devices are found at each run, so one added later follows; driving a group needs
  control of the whole type or room.

### Changed

- Modules › Airzone no longer shows the installations of another Breezone server, nor a way to add
  one: servers are linked, and what they share chosen, in Settings › Servers only.
- Solar: the “Production per panel” chart goes back day by day, like the energy curve.
- Automations and scenes: a shutter sent to 0 % reads “Close”, to 100 % “Open”, and “Open to 40 %”
  in between, instead of “Open to (Opening 0 %)”; a relay reads “Switch on” or “Switch off”.
- In the automations, a device of a module can be driven by any user who controls it (fine-grained
  rights), not only by an administrator.

## [0.11.0-beta.31] - 2026-11-17

### Added

- Fine-grained rights: the devices of the modules (Shelly, Hue, shutters, ventilation…) are given to
  a user apart from the installations — all of them, those of a room, of a type or one by one,
  read-only or with control (the widest wins), in Settings › Users › Access and rights. A device not
  given is not shown; a device given read-only is not driven, nor by a scene or an automation. The
  energy of the home needs the Energy or Solar type. Kept in the configuration backup.

### Changed

- A new user is given no device of a module by default (as for the installations); the users there
  were keep seeing them all, read-only.
- Overview: the curve of an energy block stays inside its card instead of being cut by its corners.
- Modules › Airzone shows only the installations this server reaches on its own network: one shared
  by another Breezone server is shown as such, with a link to Settings › Servers, and is no longer
  counted, edited or added from there.

## [0.11.0-beta.30] - 2026-11-17

### Added

- Overview: a block with the price of electricity now, the next 24 hours and the Tempo days, for
  an installation of the server that has the tariff.
- Every page becomes a dashboard, starting with Energy: “Edit” lets one move, resize, remove and
  set up its parts (figures of the period, live flows, curve of the day, energy day by day, breakdown
  by use, measured devices, surplus steering, electricity price), for oneself or, for an
  administrator, for everyone, as on the overview. These parts can be added to the overview too,
  listed by page. The figures may show a curve in each tile and their amounts in euros (with a
  tariff).

### Changed

- Edit mode: the blocks are outlined with a dashed line.

### Fixed

- Electricity: each installation (the home, an office…) has its own contract and prices, set in
  Settings › Electricity for the installation chosen; the energy costs, the price block and the
  automations follow them. What was set goes to the first installation of the server.
- Tempo: the colours of the days are read as soon as a contract follows them, instead of up to half
  an hour later.

## [0.11.0-beta.29] - 2026-11-17

### Added

- Energy: what the home cost (each quarter of an hour at its price, plus the day's share of the
  subscription) and what the solar panels earned (production used on the spot, and what was sold at
  the buy-back price), for the day, week, month or year shown. The price now sits at the top of the
  page.
- Tempo: the colour of today and tomorrow, and the red and white days left in the season, read from
  the public relay api-couleur-tempo.fr (RTE's data, no account) while a tariff follows colours; the
  prices and costs of each day follow its colour.
- Electricity, EDF's regulated offers (Base, Off-peak hours, Tempo): the prices come from the open data
  of the CRE, for the subscribed power, with every past grid on its date (back to 2016), read again
  every week; the off-peak hours of the home are set in the contract. A custom offer starts from the
  prices shown.
- Automations: the price of electricity now (“when the price goes above…”, “if the price is below…”
  for the off-peak hours) and the colour of today's and tomorrow's Tempo day (“when tomorrow is
  red”), with a recipe that warns of a red day.

## [0.11.0-beta.28] - 2026-11-17

### Added

- Settings › Electricity (administrators): the contract of the home (country, supplier, subscribed
  power, offer) and its prices, set visually. An offer (Base, Off-peak hours, Tempo, Custom) fills
  the grid with its slots and indicative prices to check on the bill. Each profile of day is a bar
  of 24 hours: tap a slot for its name and price, drag a limit (by half hour), split or merge. The
  calendar picks a profile per day of the week, or per announced colour, and when the day starts.
  New prices start on a chosen day; the history lists them. The price now and the next 24 hours sit
  at the top.

## [0.11.0-beta.27] - 2026-11-17

### Changed

- Overview, energy blocks: one figure with its curve of the day at the bottom, or several figures
  side by side in one block (solar, home, battery, grid), as chosen in its settings.
- Overview, settings of the blocks: only what the home has is offered (no battery without one; the
  solar surplus, charts and scenes only when there are some), and a flow it has no measure of is
  left out of a block.
- Overview, charts: several curves of a device in one chart (temperature, setpoint, humidity…), and
  the outdoor temperature, added as chips; an air-conditioning zone may show its comfort day by day
  instead. Charts are compact (the comfort of the week in one small chart), or as tall as on their
  page, as chosen in their settings.

- Electricity price (server): the contract of the home (country, supplier, offer, subscribed power)
  and its dated tariffs (subscription, profiles of day cut into slots of clock time, a calendar by
  day of the week or by announced colour, a day that may start at 06:00 as Tempo's), the price now
  and the stretches of the next hours (`/api/energy/tariff`, `/api/energy/contract`,
  `/api/energy/tariffs/{day}`, `/api/energy/price`); kept in the configuration backup.

### Fixed

- Overview: an installation without energy measures (only air conditioning, or on another server)
  no longer shows the solar, home and grid figures of the home, nor its solar surplus.

## [0.11.0-beta.26] - 2026-11-17

### Changed

- Overview: a room card only shows (a device is driven from its own block): each line says the state
  of its device (a shutter drawn at its position, a light or the air conditioning lit), and may show
  its curve over 24 hours. A device block is the full card of its page (a shutter, a light, a zone, a
  valve of the ventilation), with its controls. A chart offers only the measures that have a history.
  The lists of devices and rooms are grouped by type, with their icon.
- More icons for the rooms (kitchen, vegetable garden, terrace, workshop, fireplace, pets…), and
  names that suggest them.

### Fixed

- Overview: a shutter's line showed nothing of its state.
- The history of an Airzone zone is read like that of any device (`/api/devices/{id}/history`): its
  temperature, humidity and setpoint. `GET /api/devices/charted` lists the devices with a history.

## [0.11.0-beta.25] - 2026-11-16

### Added

- Overview of an installation (menu « Vue d'ensemble »), as a dashboard of blocks: energy figures
  (solar, home, battery, grid), the solar surplus, a card per room or per type of device with its
  switches, a single device, a chart of a measure over 24 h or 7 days, the weather, the scenes.
  Without any set, it is composed from the rooms and devices, by room or by type. « Modifier »
  arranges it: blocks dragged into place (or with the keyboard), a quarter, half or full row,
  settings, added or removed. Each user saves their own; an administrator of the installation saves
  the one everyone sees, and a user can go back to it. The defaults go with the configuration backup.

## [0.11.0-beta.24] - 2026-11-16

### Changed

- Ventilation: a room is judged as the Renson app judges it, by the unit's own index of the room
  (up to 40 good, above 100 bad), with its cause (`air_quality_index`, `main_pollutant` in
  `ventilation.room@1`). Every room was "bad" as soon as its humidity passed 75 %.
- Ventilation: the temperature of a room is no longer shown: the Healthbox measures it at the unit,
  on the extract air (cold or hot in the attic). It keeps its history, named "Temperature at the unit
  (extract air)".

### Fixed

- Renson Healthbox, with the API key: the VOC of each room are also read from the value Renson
  documents (`voc_calc_embedded`), not only from `concentration` as firmware 2.6.9 gives it.
- Ventilation: the VOC of a room show as a whole number (one decimal under 10 ppm), like the CO₂.
- Renson Healthbox, « Diagnostic »: it holds the unit's raw answers (each sensor, each room's own
  air quality index), without the address of the home. It held nothing.
- HomeKit: a model that already names its maker no longer repeats it ("Legrand Legrand Ecometer").

## [0.11.0-beta.23] - 2026-11-16

### Fixed

- Modules, « Rechercher »: it says what the network announces of the device. Announced at the same
  address but refusing connections: restart it; announced as not paired (it was reset): pair it
  again. It said "not found on the network" whenever the module did not answer.

## [0.11.0-beta.22] - 2026-11-16

### Changed

- Activity shows the installation chosen only: its state, its events, and those of no installation
  (the server, security, presence…). Timers and scenes now say which installation they act on (a
  scene across installations or with devices of the modules stays shown everywhere).

### Fixed

- HomeKit: an accessory that closes the connection says why. Closed when Breezone shows its key, or
  refused then: the accessory no longer knows Breezone's pairing ("remove this module, then pair it
  again"); closed before: it may be restarting or have too many connections open. It said only
  "Connection reset by peer".

## [0.11.0-beta.21] - 2026-11-16

### Added

- App, Lights: a switch per room turns all its lights on or off.
- App, House: the Charging tile shows a car while a vehicle is plugged in, a bolt while it charges,
  a grey plug while the charger is free.
- Automations, "Afterwards — when it is no longer true": a rule started by a measure, a state, the
  sun on a façade or presence can do something when that trigger no longer holds (back past its
  threshold with the margin, for as long as it took to start): put back as before, or other steps.
  Steps of "Then" still under way stop. Shown in the app.
- Automation recipes "Solar surplus: heat a room" and "Solar surplus: cool a room": they follow the
  mode of the air conditioning instead of setting it.
- Scenes drive every module, not only the Airzone zones: a light, a shutter, a relay, the charger…
  with the same actions as the automations ("+ Device" in the scene editor on the web). A delayed
  action on a device becomes a timer. The app runs these scenes and keeps their device actions when
  it changes their zones. Devices of the modules stay for administrators.
- Each device says whether the signed-in user may drive it (`can_control` in `/api/devices`): the
  web and the app grey out the controls of a device they may not drive, instead of failing.
- Automations written by a newer version (then the server went back) are named to administrators
  (`GET /api/automations/unreadable`), on the web and in the app, and can be deleted. They were
  hidden without a word.
- The app is complete in Spanish (135 texts were shown in French).

### Changed

- The weather and the cities are looked up on the internet only when `BREEZONE_WEATHER` is on: on
  by default in a release build (the Docker image), off in a development build and the tests, so
  that working on the server sends nothing to Open-Meteo or MeteoAlarm. `GET /api/weather` tells it
  (`online`).
- Automations, "put back as before" leaves a device someone changed in the meantime as it is, and
  the history says so.
- The stored secrets (Airzone Cloud password, Hue keys, HomeKit pairings, this server's
  federation key) are encrypted with a key of their own, `data.key` in the data directory, to keep
  with the database. Changing `BREEZONE_JWT_SECRET` to sign everyone out no longer makes them
  unreadable. An existing server writes the key it had into `data.key` at its first start (nothing
  is encrypted again, an older version still reads them); a damaged `data.key` stops the server
  instead of being replaced.
- The main views of the API are described in `server/contracts/views.json`, checked against the
  real answers of the server; the web types are generated from it and the app's models are tested
  with it.

### Fixed

- Zone history: heating and cooling periods, when the installation does not record its demand time,
  are read on the temperature: a zone heats while its temperature rises from below its setpoint (and
  cools while it falls from above it), from the trough to the top. They were guessed from a 0.5°
  gap to the setpoint, which a well-regulated zone never reaches: the web showed none, the app showed
  thin bands at the tops of the curve, the wrong way round.
- App, House: a paused charger, which still says it is charging at 0 W, shows as paused.
- Automations: the values of a state are in words ("Vehicle plugged in", not "plugged"), and the
  number fields of an action show their name in full.
- A paused charger shows the same state everywhere: the app's Charging screen said "Charging" and
  the web's Modules tab ignored the pause.
- Numbers on the Modules tab and the solar surplus follow the language of the app, not the browser's.
- A failed read of the holiday pause no longer lets the automations start.

## [0.11.0-beta.20] - 2026-11-15

### Added

- Solar surplus steering (Energy page): what would go back to the grid runs the charger and the
  plugged loads of the home (water heater, pool pump…) in the order of their priority. The charger's
  current follows the sun between a minimum and a maximum; a load starts after a minute of surplus,
  stops after three minutes without (a passing cloud does not stop it), and a relay stays on at
  least the time set. Each start and stop is in the activity feed.
- Charging page: a charging mode, Manual, Solar or Solar + guarantee ("+20 kWh before 07:00": when
  the sun can no longer make it, the charger charges at its maximum from the grid, just in time).

### Changed

- Automations come first on the Schedules page, above the Airzone schedules.
- Automation editor: a short line says that "and" is a condition (If), next to "Add a trigger (or)".
- Lights: only the selected light is framed by its colour, while it is lit in colour.

## [0.11.0-beta.19] - 2026-11-15

### Added

- Automations go further, without becoming a programming language:
  - several triggers (any one starts the rule); a device that changes state (the charger starts
    charging); dawn and dusk; the sun reaching a façade; the weather (rain within the hour, rain,
    wind, tomorrow's low and high);
  - conditions joined by "and" or "or", with groups ("A and (B or C)"); the sun on a façade;
  - "wait", "wait until… (at most…)", and "put back as before" (settings, charge, boost);
  - an "else" branch, run when the conditions are false;
  - runs in the background, with what to do when the rule starts again while it waits (ignore or
    start over);
  - the history of each rule: its last 20 runs, and why it did not run ("conditions not met: living
    room 19.1 °C");
  - ready-made recipes built with your devices (solar surplus to the car or a room, shutters at
    sunset and sunrise, heat protection, window open, rain coming, light at dusk, ventilation boost,
    frost coming), and "duplicate".
- Automations, the rest of the plan:
  - the buttons of a remote start a rule (any button or one of them, single, double or long press);
  - presence: each user says "I am home" (web and app); "the first one gets home", "the last one
    leaves" and "someone is home" in the rules;
  - "Notify": the activity feed, a notice in Breezone while it is open (web and app), and a phone
    notification through ntfy (free, no account, can be self-hosted), with a test button;
  - the weather warning of the home's area (Météo-France vigilance through MeteoAlarm), from green to
    red;
  - holiday pause: no rule starts by itself until it is switched back off;
  - rules that may contradict each other (same device, another setting, at the same time) are flagged
    on both cards;
  - the app shows the automations of an installation (Schedules tab): the rule as a sentence,
    switch on or off, try, history, "I am home"; they are still created and changed on the web.
- Lights: a coloured light that is on is framed by its colour.
- Automations: the outdoor humidity, as a measure of the weather.

### Changed

- The weather follows the installation chosen (web and app): each installation has its own weather,
  sun, weather warnings and outdoor temperature curve. Its city is looked up once, when it is set (by
  hand, from the Airzone account or from another server), and kept with the installation; a rule uses
  the weather and the sun of the installation it drives. The city set for "the home" in earlier
  versions goes to the first installation.
- The ventilation is shown in a pale sky blue instead of violet (web and app).
- Automation editor: the thresholds follow the measure chosen (no more "500 °C"), and the lists of
  devices are grouped by type with an icon.

## [0.11.0-beta.18] - 2026-11-15

### Added

- Automations “when… if… then…” (Schedules page, next to the Airzone schedules): a rule reads like a sentence and runs on the server.
  When a measure crosses a threshold for some time (power returned to the grid, solar production,
  home consumption, outdoor temperature, the temperature of a room…), at a fixed time, or at sunrise
  or sunset; if up to three conditions are true (a measure, the state of a device, a period of the
  year, hours, days, day or night); then up to five actions on any device (a zone, a charger, a
  relay, a light, a shutter…) or a scene. Example: when more than 500 W go back to the grid for
  5 min, if the living room is below 18 °C from November to March, heat the living room and charge
  the car. Each run is in the activity feed with its reason.

### Changed

- A shutter that cannot be stopped on the way has no stop button any more, on the web and on the
  phone, instead of a button that answers with an error. This is the case of Legrand shutters
  through HomeKit: their gateway has no “hold position” and only updates the position at the end of
  the move (Home Assistant hides the button too). The new optional `stoppable` field of
  `cover.position@1` tells it; without it, the button stays.
- A shutter moves on the screen as soon as it is told to, on the web and on the phone: the window
  goes down or up at the pace of a real shutter (about 25 s from closed to open) and the slider
  stays on the position asked, instead of going back to where the shutter started until the
  gateway tells the end of the move. What the hardware tells wins as soon as it tells it.

## [0.11.0-beta.17] - 2026-11-15

### Added

- Settings › Rooms: each room has an icon, guessed from its name (sofa for the living room, bed for a
  bedroom, bath, kitchen, garden…) or chosen among 24; shown on the Lights and Shutters pages too.

- The city and country of an installation can be edited (Settings › Installations › an
  installation): the weather and the sun follow them at once, and the Airzone account no longer
  replaces a city set by hand (emptied, it goes back to the account's). The country helps to find
  the right city among those of the same name.

### Changed

- Programmations and Routines follow the installation chosen: its Airzone schedules (new ones for
  it), its scenes, its timers and its Airzone routines only, no longer those of every installation.

## [0.11.0-beta.16] - 2026-11-15

### Added

- Periods back to five years wherever a curve goes over time: Énergie (« 5 ans », by month),
  Solaire (30 days, 1 year, 5 years, by month), the charger and the ventilation (24 h to 5 years),
  the history of a zone (« 1 an », « 5 ans », a point per day). A longer period shows only when the
  history goes back further than the shorter one. In the app too: zone history, charger,
  ventilation. The days of the Airzone zones are kept for good, as those of the devices.
- Weather and sun (roadmap F9): sunrise and sunset computed by the server; the weather of the home
  from Open-Meteo (no account; the city of the first installation, turned once into coordinates)
  every 30 minutes. A weather card on « Mes installations » and on the house screen of the app,
  and the outdoor temperature on the history of the zones (`GET /api/weather`,
  `GET /api/weather/history`, `PUT /api/weather/place`).
- Settings › Modules › a module › « Diagnostic »: a file of what the hardware says about itself
  (the database of a HomeKit accessory…), to understand how a model behaves; without any secret.

### Changed

- The settings and fixed values of the hardware (limits of a charger, whites of a lamp, nominal
  flow of a valve, target of a shutter…) are no longer written to the history every few minutes.

### Fixed

- HomeKit shutters without « hold position » (Legrand): « Stop » sent the shutter to its current
  position, which the gateway updates only at the end of a move: the shutter went back where it
  started. Such a shutter now says it cannot be stopped through HomeKit.

## [0.11.0-beta.15] - 2026-11-15

### Fixed

- Zone history (web and app): without a recorded demand time, a zone heating while warmer than its
  setpoint was drawn as « Chauffage ». Heating is now estimated only when the room is too cold,
  cooling only when it is too hot.
- Zone history (web): the zoom bar under the chart shows an outline of the temperature instead of
  an empty strip.

### Changed

- History goes back years and the database stays small: every hour, the detail of the devices
  (every few minutes) is summed up per hour (kept about a year) and per local day (kept for good),
  and the energy of each day is kept once computed. A curve reads the summary that fits its period
  (`GET /api/devices/{id}/history` up to five years, with `oldest`, the oldest point;
  `GET /api/energy/days` up to five years; `GET /api/energy/range`, the first day with energy). Old
  rows are deleted in small batches, so that the cleanup no longer holds the database.

### Fixed

- Énergie › En ce moment: a charger is a consumption of the home without being given a role, even
  when idle, unless a meter on its line already measures it. A measure left as « Simple mesure »
  now says it is not counted.

### Added

- Mobile app: Énergie (the home now: solar, home, battery, grid; today's curve with what was
  produced, used and self-used; the uses now), Solaire (production now and today, each inverter and
  its panels) and Recharge (state, power, session and goal, pause and start, current, dynamic mode,
  lock). The house screen shows their tiles in the order of the web menu.

### Changed

- Solaire › Production par panneau: one panel in two is dashed, so two panels giving the same power
  (sharing an input of the inverter) both show instead of one hiding the other.
- Ventilation (web and app): without the Renson API key, the page says that the room measures come
  with the key, and where to add it.

- Android: the debug build is « Breezone Debug » (`com.breezone.app.debug`), installed next to the
  store version with its own data.

## [0.11.0-beta.14] - 2026-11-15

### Added

- Mobile app: the house screen of the mockups. Opening an installation whose server has modules
  shows its types (Climatisation, Lumières, Volets, Ventilation) with what each does now; without
  modules, it opens the climate as before. New screens:
  - Lumières: the Hue scenes, « Tout éteindre », the lights by room with their switch and
    brightness; the detail of a light with its big brightness bar, white temperature and colors;
  - Volets: « Tout ouvrir », « Tout fermer », the shutters by room with ▲ ■ ▼; the detail of a
    shutter with its window and its position bar to drag;
  - Ventilation: the level and flow of the house, the room most in demand and why, the boost of the
    house; a valve with its flow and humidity over 24 h, its boost (30 min, 1 h, 2 h) and profile.
  The pages of modules follow the installation chosen, as on the web: an installation reached
  through another Breezone server does not show this server's modules.

### Fixed

- The menu follows the installation chosen: with an installation reached through another Breezone
  server, the pages of this server's modules (Énergie, Lumières, Volets, Ventilation, Solaire,
  Recharge) are hidden, and opening one says they are this server's, instead of showing them as if
  they were that installation's.
- Adding a module: one spinner while the network is searched, on « Rechercher » (the line below no
  longer has its own).

## [0.11.0-beta.13] - 2026-11-15

### Added

- Ventilation: the Renson Healthbox 3.0, through its local API (no Renson account), found by the
  network scan. A « Ventilation » page shows the level and air flow of the house, the room most in
  demand and why (humidity, CO₂, VOC), and a card per valve with its air flow, profile and history;
  boost a room (30 min, 1 h, 2 h) or the whole house, change a room's profile. With the API key Renson
  gives on request (Settings › Modules › the Healthbox), the humidity, CO₂, VOC and temperature of
  each room are read too; the unit keeps the key, Breezone does not store it. The rooms of the
  Healthbox can be taken as Breezone rooms. New traits `ventilation.unit@1` and `ventilation.room@1`.
- HomeKit remotes (the Legrand 4-button remote…): found with the accessory, listened to on a session
  of their own, with the last press shown and each press noted in Activity (button, single, double
  or long). Breezone triggers nothing: what a button does stays set in the maker's app, so a press
  is never acted on twice. New trait `button@1`.

## [0.11.0-beta.12] - 2026-11-15

### Added

- Activity: an « Erreurs » tab with the server's own last errors (a request that failed: what was
  asked, when, and the cause, « database is locked » for instance) and the events in error or
  warning, each with its details unfolded on demand (`GET /api/system/incidents`).
- « Climatisation » in the menu, first of « Maison », as in the mockups: the zones of the installation
  chosen. « Mes installations » only lists the installations now.

### Fixed

- « database is locked » on a NAS when many changes were made at once: the database commits without
  waiting for the disk (WAL with `synchronous=NORMAL`), reading the devices no longer writes, a node
  seen again is written at most once a minute, and a write waits up to 10 s for its turn.

### Changed

- The installation chosen, under « Installations » in the menu, is a small label (no link).

## [0.11.0-beta.11] - 2026-11-14

### Changed

- The installation chosen in « Mes installations » shows in small under « Installations » in the
  menu. Settings › Rooms only lists the zones of that installation (with the modules of this server);
  for an installation reached through another Breezone server, it says its rooms are set there.
- The room of an object, and the role or measure of an energy channel, show the choice at once with a
  spinner until it is saved, instead of the former value for a moment.
- Lights page: the detail of a light follows the mockups: its room and brand under its name, the big
  brightness bar filled with its light (drag it, or use the arrow keys), the white temperature as a
  warm-to-cool gradient with its value in kelvins, and the color swatches.

## [0.11.0-beta.10] - 2026-11-14

### Added

- Settings › Rooms: the rooms of the house (create, rename, order, delete; the objects of a deleted
  room stay, without a room), and every object of every module (Airzone zones by installation, Hue
  lights, HomeKit shutters…) with its room, « sans pièce seulement » to see what is left to place.
  For Hue, its own rooms are taken from there too.
- A room is created from the room choice itself (« Nouvelle pièce… »), in Settings › Modules and on
  the Shutters page: until now no screen could create one, so only « Sans pièce » was offered.

## [0.11.0-beta.9] - 2026-11-14

### Added

- Rooms of the devices of modules: each device gets its room in Settings › Modules (and a shutter in
  its detail on the Shutters page). « Reprendre les pièces Hue » takes the rooms of the Hue app:
  the missing ones are created by name, the lights not in a room yet go in theirs.
- Hue scenes on the Lights page: those of a room under it, those over several rooms above; one click
  turns one on (`GET /api/scenes`, `POST /api/scenes/{source}/{scene}`).
- Shutters page (web), as in the mockups: shutters by room with « Tout ouvrir » and « Tout fermer »,
  a card per shutter showing how far it is open with ▲ ■ ▼, and the detail of the one chosen
  (position, open, stop, close, its room). In the menu when a module provides shutters.

### Changed

- HomeKit: a device takes its accessory's name (« Rolling shutter switch 1 ») rather than its
  service's (« Window covering »), and its kind says what it is: shutter, roof window or door
  (shown in Settings › Modules).

## [0.11.0-beta.7] - 2026-11-14

### Fixed

- HomeKit pairing of Legrand accessories (Home + Control gateway, Ecometer): they refused Breezone's
  controller identifier, which was not a UUID, and the pairing ended in « Code incorrect » although
  the code was right. Breezone now gives a UUID, as Apple's controllers do. A refusal after the code
  is accepted no longer says the code is wrong.

## [0.11.0-beta.6] - 2026-11-14

### Added

- Export and import of the configuration (Settings › Breezone), to restore a server or move it to
  another machine: installations, modules, devices and rooms, routines and timers, users and their
  rights, a few settings. The file holds no secret (passwords, Airzone account password, HomeKit
  pairings, Hue key, federation keys) and no history. An import replaces that configuration; on the
  same server, the secrets and history of what comes back are kept; elsewhere, a list says what to
  type again.

### Fixed

- A HomeKit pairing started and never finished no longer keeps the accessory busy: it is given up
  after 2 minutes, or as soon as the add dialog closes. An accessory busy with another pairing says
  so instead of « Réponse inattendue ». The `Host` header no longer carries the port, as the
  reference controllers send it.

## [0.11.0-beta.5] - 2026-11-14

### Changed

- Settings: the Nodes and Shares tabs become a single Servers tab, in three sections: « Ce serveur »,
  « À qui ce serveur est partagé » and « Quels serveurs partagent avec celui-ci ». Messages that
  pointed to Settings › Shares or Settings › Nodes now point to Settings › Servers (the old
  `?tab=nodes` and `?tab=shares` links still open it).
- Switches waiting for the server (lamps, relays, charger lock, Airzone schedules, beta channel)
  show a spinner in their thumb instead of turning grey with a “forbidden” cursor.

### Fixed

- HomeKit pairing of an accessory with a screen (Legrand Ecometer): such an accessory shows a new
  code each time a pairing starts, so the code typed beforehand was always refused. « Appairer » now
  starts the pairing first, then asks for the code shown; a wrong code starts a new pairing.
- The scanner no longer offers to pair a Hue bridge through HomeKit: it also answers as a HomeKit
  accessory, and its own driver is found at the same address.

## [0.11.0-beta.4] - 2026-11-14

### Added

- Philips Hue: the bridge is found by the scanner and added with its button (its local API, no Hue
  account), each lamp becomes a `light` device (new trait: on, brightness, white temperature, color,
  only what the lamp has). Lights page (web), as in the mockups: lamps by room with « Tout allumer » and
  « Tout éteindre », a card per lamp tinted with its light, and the detail of the lamp chosen
  (brightness, white temperature, colors).
- HomeKit controller: Breezone pairs with a HomeKit accessory or bridge (a Legrand Home + Control
  gateway, a VELUX gateway…) with its 8-digit code, then reads and drives it on the local network
  through encrypted sessions, without Apple Home or any account. The scanner finds the accessories
  (mDNS, asked to the group and to each address), « Ajouter un module » pairs them. Shutters, windows
  and doors are `cover.position` devices (new trait: opening, target, movement; open to, stop),
  outlets, switches and lights `switch.relay`. Pairing changes nothing else on the accessory;
  removing the module removes Breezone's own pairing, and only it.
- A module whose address changed is found again: each source keeps the identity of its hardware
  (Shelly id, Trydan id, Hoymiles serial number), learnt at its first reading when it was added by its
  address. Unreachable for 3 minutes, it is looked for on the network by this identity (at most every
  15 minutes), and its address follows; the move is in the activity. « Rechercher » in Settings ›
  Modules does it at once (`/api/sources/{id}/locate`).

## [0.11.0-beta.3] - 2026-11-14

### Added

- Solar page (web), for the detail: the production now and today, self-consumption and what is
  returned, each inverter with its temperature, voltage and frequency, its panels with their power,
  voltage, current and production, the power of each panel along the day (a curve per panel), today's
  production beside the consumption, and the last 30 days. The energy page keeps the solar production
  (figures, flow, curve, days) and leads to it.
- Device history over a period (`/api/devices/{id}/history?from=&until=&bucket=`), e.g. a local day.
- The menu in sections, as in the mockups: Home (energy, solar, charging), Automations, Server
  (modules, settings).

### Fixed

- A solar inverter is the solar production of the home without being told; a meter on its line
  (a Shelly channel) can say which inverter it measures, and both count once. A measure linked to an
  inverter counts as a production, whatever role it was given; a production that does not say what it
  measures leaves the inverters out unless they are told to count (another installation).
- A source that misses a reading (a Hoymiles DTU serves one client at a time) stays online with its
  last values for a minute, or three readings, instead of showing as unreachable.

## [0.11.0-beta.2] - 2026-11-14

### Changed

- Settings › Modules (web), as in the mockups: the list of modules on the left, Airzone first, and
  the detail of the chosen one on the right: its connection (address, search, test, state, firmware,
  last reading), who sets the charging current for a charger, its devices, and what the module reads
  and writes. Airzone is a module like the others: its installations and the Airzone account are in
  its detail; the « Compte Airzone » and « Installations » tabs are gone (old links still lead there).
- Energy page (web): the products measuring in the header, a Year period (bars per month), CSV export,
  and with a solar production, the bars show it used on the spot or returned, as in the mockups.
- Charging page (web): the charger and its maximum power in the header with when it was read, goal
  and estimated end, the solar surplus beside the charging power, charger settings with a link to the
  module, and the states shared by every charger.

## [0.11.0-beta.1] - 2026-11-14

### Added

- Hoymiles micro-inverters through their Wi-Fi DTU (DTU-W100, DTU-Lite-S, DTU-WLite, DTU-Pro), read
  locally every 30 seconds, found by the network scanner. Each inverter gives its AC power, today's and
  total production, temperature and grid frequency, and can be the solar production of the energy page;
  each panel gives its power, voltage, current and production, shown on the energy page. Read only.
  New traits: `solar.inverter`, `solar.panel`.
- V2C Trydan charger (local API), found by the network scanner and added in one click. New trait
  `ev.charger`: state, whether a vehicle is plugged in, session energy and duration, current; actions `start` (with a maximum current
  and an energy to add, after which Breezone stops the charge), `stop`, `set_limit`, `lock` and
  `set_auto_limit` (the charger sets its current itself, V2C dynamic mode). The charger also measures
  its power, so it can be a consumption on the energy page.
- Charging page (web): the state of each charger, start with a maximum current and an energy to add,
  stop, lock, who sets the current (the charger or Breezone), and its power over the last 24 hours.
- Drivers (ADR 0013): a common interface for every source of devices, with Shelly as the first one
  (Plus, Pro, Gen3, Gen4, local RPC API): a Pro 3EM gives a three-phase meter, plugs and relays give
  switchable relays that measure their power. Sources are configured by administrators
  (`/api/sources`), read every 5 seconds, and their devices are listed with the zones in
  `/api/devices`. A single route applies an action of a device's trait whatever its source
  (`/api/devices/{id}/actions`): it drives an Airzone zone as well as a Shelly relay. New traits:
  `energy.power`, `energy.meter`, `switch.relay`.
- Network scanner for every supported brand (`/api/discovery/products`): each address is asked at once
  whether it is an Airzone WebServer or a Shelly, and products are identified down to the model
  (« Shelly Pro 3EM, monophasé : 3 voies indépendantes »), with those already added marked.

- Settings › Modules (web): the sources of devices with their state, « Ajouter un module » that
  scans the network and adds a Shelly in one click (or by its address), and each energy measure
  named and given a role: grid (whole home), solar production, battery, or the consumption of a
  device or a room. The energy page will rely on these roles.
- Device history: every number of the state of a device (power, energy, relay…) is recorded every
  5 minutes, as the mean of the readings since the previous point, and served per device
  (`/api/devices/{id}/history`). Kept as long as the zone history.
- Energy page (web): the home right now (solar, battery, grid, home and its main consumers, with
  flows that move the way the energy goes), the curve of a day every 15 minutes, the energy per day
  over 7 or 30 days, where the energy goes, and the measured devices with their relay. Production,
  self-consumption and self-sufficiency appear with a solar channel and a main meter. Served by
  `/api/energy/live`, `/api/energy/day` and `/api/energy/days`; the page shows in the menu once a
  measure has a role.

### Fixed

- Network scan: devices were missed when many addresses were asked at once (a real device can take
  several hundred milliseconds to accept a connection then); it waits longer per address and asks
  more of them in parallel, about 3 seconds for a /24. The Airzone scan no longer reads every zone
  of each WebServer found before answering.

### Added

- Home model (ADR 0013), with nothing changed in the interface yet: Airzone zones are now also
  devices of the trait `climate.zone@1`, with a stable id, and can be named and placed in rooms
  (`/api/devices`, `/api/rooms`). The catalog of traits (`server/contracts/traits`, served at
  `/api/traits`) is the single contract from which the web and mobile types are generated.

### Changed

- Zone page (web and app): the air quality section only shows when the thermostat has measured the
  air quality now or in the last 24 hours (it was 30 days), so a sensor that stopped measuring does
  not leave an empty section for a month.

## [0.10.1] - 2026-11-14

### Added

- Servers of different versions now tell each other what they can do (ADR 0012), when their link
  opens and again after an update, a rollback or any change. Settings › Nodes and Shares show the
  other server's version (with a « bêta » badge) and whether they understand each other fully,
  partly (with what is missing and which one to update) or no longer. `GET /api/capabilities` gives
  the same for clients; its fingerprint is in `/api/version`.

### Changed

- A zone command with a field the server does not know is refused (`422`, `code: "unsupported"`)
  instead of being applied in part, so that the sender, an app or another server, knows this server
  must be updated. Commands to another server are checked against what it announced before they are
  sent.

## [0.10.0] - 2026-11-13

### Added

- Beta channel: Settings › Breezone › « Recevoir les versions bêta ». A server on it is offered the
  pre-releases (`v1.0.0-beta.1`…) as well, installed from the web UI like any version (signed, undone
  if it does not start); a « bêta » badge marks them. Leaving the channel never goes back to an older
  version.
- Releases: a pre-release tag (any branch, e.g. `beta`) publishes a GitHub pre-release and the
  `:beta` Docker image only; `latest` and stable versions only come from main, and a stable tag
  outside main is refused.

### Fixed

- Web UI: after an update from Settings › Breezone, the page did not reload once the server was back,
  and kept the previous interface. It now waits for the new version, and any open page reloads by
  itself when the server's version changes (checked every minute), after an update from the web UI
  or a new Docker image alike.

## [0.9.1] - 2026-11-13

### Fixed

- Settings › Breezone: a release older than the installed version (an image installed before its
  GitHub release was published) was shown as the latest version; « Breezone est à jour » now shows
  after any check that finds nothing newer.
- Zone page (web and app): the air quality section only shows when the thermostat has measured the
  air quality, now or in the last 30 days.

## [0.9.0] - 2026-11-12

### Added

- Update Breezone from the web UI (#40): **Settings › Breezone** shows the installed version and the
  latest release (checked once a day), and administrators install it in one click. The server and the
  web UI are downloaded from the official repository only, their Ed25519 signature checked against
  the project key compiled in, the database backed up, then the server restarts on the new version in
  a few seconds, without Docker. A version that does not start is undone at the next start (database
  restored, previous version). `BREEZONE_SELF_UPDATE=false` removes the button. The app tells
  administrators when their server has an update. Each release now publishes signed
  `breezone-<version>-linux-<arch>.tar.gz` archives.
- Zone history: heating and cooling periods as bands under the curves (web and app), from the
  recorded demand time, else estimated from the gap to the setpoint.
- `breezone-server --version`.

## [0.8.0] - 2026-11-12

### Added

- Charts (#30): a 24-hour temperature curve on every zone card (web and app), and the comfort of a
  zone day by day on its page: minimum, maximum and mean temperature, share of the time on within
  ±0.5° of the setpoint, time on, over 7 or 30 days.
- Zone history: pinch or drag to zoom (app) or zoom bar (web), touch or hover for every value at that
  time, tap the legend to hide a curve; the air quality of the zone shows as a colored band under the
  curves, and a gap without readings breaks the lines instead of joining them. The comfort chart
  also shows the mean humidity of each day. Charts use libraries (Recharts on the web, Vico in the
  app) instead of hand drawing.
- Air quality (#30), for the thermostats that measure it (other zones show nothing): a leaf on the
  zone card colored by level (good, moderate, poor), a band of the level under the history curves,
  and on the zone page the level now, the purification mode and the share of each level day by day. Read from the zone data the WebServer already sends; its `iaq` endpoint, which
  stops some WebServers from answering, is never called.
- Demand time: an installation can record how long each zone asks for heat or cold, with the
  history (installation settings, off by default). It is set on the server that owns the
  installation; the servers it is shared with follow it. Shown on the comfort chart, it prepares
  the energy features.
- API: `GET /api/installations/{id}/trends`, `GET /api/installations/{id}/zones/{s}/{z}/daily`,
  `record_demand` on installations, `air_quality` on zones, `demand_secs` and `air_quality` in the
  history.

## [0.7.4] - 2026-11-09

### Fixed

- Installations reached through another Breezone server: the latency shown counted the wait of the
  live link (up to 45 s) when the content changed meanwhile, e.g. « 14986 ms » for a fast network.

## [0.7.3] - 2026-11-09

### Fixed

- Airzone schedules of an installation reached through another Breezone server: creating, changing,
  switching on or off and deleting them now goes through the server that owns the installation,
  which writes them with its own Airzone account. Before, this server's account was used, and
  Airzone refused it without a subscription (« An active subscription is required »).

## [0.7.2] - 2026-11-09

### Fixed

- Airzone schedules: switching a schedule on or off failed with « Bad input parameters » when it
  has a setpoint. Breezone sent the schedule as read from Airzone Cloud (setpoints as
  `{celsius, fahrenheit}`, read-only fields); it now sends what the official app sends.

## [0.7.1] - 2026-11-09

### Changed

- Users: rights are given per installation — no access, read-only, control (with or without the
  right to change zone names and groups, routines, Airzone schedules) or administrator of the
  installation (every right, plus its settings: address, Airzone link, deletion, from the
  installation's page). Rights given until now apply to each installation of the user. Server
  administrators keep every right.
- Web and site: link to the Android app on Google Play.

- Web and app: changing the mode of a system asks for a confirmation first (it applies to every
  zone, an accidental tap could switch the house from heating to cooling).
- Security: device sessions last 60 days instead of 365 (still extended at each use).
- Security: sign-in attempts are limited per account and address, per address and per account
  (an attacker elsewhere no longer locks the owner out), the counters cannot fill the memory, and
  at most two passwords are hashed at a time.
- Security: the server sends a Content-Security-Policy, `X-Frame-Options: DENY`,
  `X-Content-Type-Options: nosniff` and `Referrer-Policy: no-referrer`.

## [0.7.0] - 2026-11-09

### Added

- Users: an administrator chooses what each user may change on the installations they drive:
  zone names and groups, routines, Airzone schedules (everything allowed by default, as before).
  Driving, running a routine and switching a schedule on or off stay possible; the web and the app
  hide what is not allowed. Server settings stay reserved to administrators.
- Languages: the web UI is in French, English and Spanish, chosen in the profile (My account ›
  Language; automatic follows the browser). The server writes its messages (errors, activity feed,
  connection and synchronisation states) in the language of each reader: the web and the app send
  theirs; the activity feed is stored in the three languages.

## [0.6.1] - 2026-11-09

### Changed

- App: the section bar of an installation stays visible (it no longer hides while scrolling), the
  lists end right above it.
- App: the setpoint in the routine and schedule editors follows the limits of the chosen zones; the
  Airzone password can be shown while typing it.
- App: going back from an installation no longer shows two bottom bars during the animation (the
  section bar switches in place, only the content slides); the home screen loader is the wavy
  Material 3 Expressive one.
- App (Android): the release build is shrunk, optimized and obfuscated by R8 (2 MB APK); exception
  names stay readable in error messages.

## [0.6.0] - 2026-11-08

### Added

- Federation: a server follows the installations another server shares with it **live** (long
  polling over the signed API): a change there shows here within a second, instead of at the next
  read. At rest, a few hundred bytes every 45 s; the full state only travels when something changes.
  The link reopens by itself when either server comes back (retries after 10 s, then 1, 2 and 4 min, then every 5 min; and at start);
  servers up to 0.5.x keep being read periodically.
- Web: the version of the Breezone server, at the bottom of the side bar and in the account menu.

### Changed

- App: the Zones / Routines / Schedules sections of an installation are a bottom bar that hides while
  scrolling down; the latency is no longer shown (a green dot when connected, « Offline » otherwise);
  the selected tab label is readable again.

## [0.5.0] - 2026-11-08

### Added

- API versions: `/health` announces `api_version` and `min_client_api`; the app sends its own in the
  `Breezone-Api` header and asks to update the app or the server when they are too far apart (clients
  without the header are still served).
- App: each installation has **Zones**, **Routines** and **Schedules** tabs; the Airzone schedules of
  the installation can be viewed, switched on or off, created, edited and deleted from the app. The
  separate « Routines » tab is gone.
- Web and app: the mode selector of a system sits on the row of its group when all its zones belong to
  that group (in the installation header otherwise).

- App: built-in demo (« Try the demo » on the first screen): a simulated home and workshop, with
  live updates, history and routines, without any server nor Airzone hardware.
  The demo account `https://breezone.cloud` / `demo` / `demo` opens it too (store reviews); no request
  is sent to that address.

- App: Spanish translation (French, English and Spanish, following the phone's language; per-app
  language in Android 13+ and iOS settings).
- App: a server whose HTTPS certificate fails the usual checks can be trusted after reviewing it
  (names, issuer, dates, SHA-256 fingerprint); only that exact certificate is then accepted.

### Changed

- App: the version shown in Settings comes from the build (it showed 0.1.0).
- App: explicit connection errors (invalid certificate, unknown address, refused connection, no
  answer, no network) instead of the raw system message.
- When live updates (WebSocket) are blocked, typically by a reverse proxy without the WebSocket
  headers, the server says so in the activity feed and the app shows « Live updates unavailable »
  with the reason instead of « Connecting… ».

## [0.4.1] - 2026-11-07

### Added

- Choose which of the installations another server shares are shown on this one (Settings ›
  Nodes); installations it shares later are shown until excluded.

## [0.4.0] - 2026-11-07

### Added

- Per-user access: a user only sees the installations an administrator gives them, read-only or
  with control (web, app and live updates); new installations are given to nobody. Existing users
  keep access to every current installation.
- "Add an installation outside Airzone" also offers an installation of another Breezone server.

### Changed

- Read-only installations show their controls disabled in the web UI.
- A reciprocal share tells whether the other server shares back, and how to fix it if not.
- Linking an installation through another server shows when that server still has to accept.

### Fixed

- An installation shared by another server is no longer hidden by a local copy of the same
  WebServer that this server cannot reach (e.g. Airzone account connected on both servers); the
  local copy still wins when it answers.

## [0.3.0] - 2026-11-07

### Added

- Reciprocal sharing: an invitation can ask the other server to share its installations too; the
  invited server chooses which ones, and both directions are linked when the request is accepted.
- Servers behind an HTTPS certificate that fails the usual checks: the certificate is shown (names,
  issuer, dates, SHA-256 fingerprint) and can be trusted; only that exact certificate is then
  accepted, in both directions.

### Fixed

- Clearer errors when another server cannot be reached (certificate not covering the name, unknown
  domain, refused connection).
- A server waiting for acceptance no longer shows a wrong permission and a raw English message.
- The fake WebServers of the demo have distinct MAC addresses.

### Documentation

- Screenshots, a guide to link two sites, the older Synology Docker package, the reverse proxy
  certificate, and a minimal `docker-compose.yml` using the published image.

## [0.2.1] - 2026-11-07

### Changed

- Default port is now 8427 instead of 8080, which is often taken by other applications. Set
  `BREEZONE_PORT` (and `BREEZONE_HTTP_PORT` with compose) to `8080` to keep the former address.

## [0.2.0] - 2026-11-07

### Added

- Secure pairing between Breezone servers: single-use invitation valid 15 minutes, access request
  to accept in Settings › Shares, every request signed with the server's Ed25519 key (time window
  and replay protection).
- Visual acknowledgement of commands (pending, applied, failed) in the web UI and the app.
- Synology NAS installation guide.

### Changed

- Invitations from 0.1.0 (`bz1.`) are no longer accepted: existing links between servers are
  removed on upgrade and must be paired again.

### Fixed

- Setpoint briefly jumping back to its previous value after a change.
- Zone names and settings kept from another WebServer when an installation is pointed at a new one.
- Android 17: request the local network permission needed to reach the WebServer.

## [0.1.0] - 2026-11-06

### Added

- Rust server driving Airzone WebServers through their local API: zones, groups, modes, history,
  activity feed, users and devices.
- Web UI (React) and Android / iOS app (Compose Multiplatform).
- Schedules of the Airzone app, synchronised hourly and editable from Breezone (Airzone Cloud).
- Routines: scenes with delayed steps and timers, run by the server.
- Federation between Breezone servers (sharing installations across sites without VPN).
- Optional read-only import of the Airzone account (names, city, groups).
- Docker image for `linux/amd64` and `linux/arm64` on GitHub Container Registry.

### Changed

- Project renamed from FreeZone to Breezone (former `FREEZONE_*` variables and `freezone.db` are
  still recognised).

[Unreleased]: https://github.com/EmmanuelVinas/breezone/compare/v0.11.0...HEAD
[0.11.0]: https://github.com/EmmanuelVinas/breezone/compare/v0.10.1...v0.11.0
[0.10.1]: https://github.com/EmmanuelVinas/breezone/compare/v0.10.0...v0.10.1
[0.10.0]: https://github.com/EmmanuelVinas/breezone/compare/v0.9.1...v0.10.0
[0.9.1]: https://github.com/EmmanuelVinas/breezone/compare/v0.9.0...v0.9.1
[0.9.0]: https://github.com/EmmanuelVinas/breezone/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/EmmanuelVinas/breezone/compare/v0.7.4...v0.8.0
[0.7.4]: https://github.com/EmmanuelVinas/breezone/compare/v0.7.3...v0.7.4
[0.7.3]: https://github.com/EmmanuelVinas/breezone/compare/v0.7.2...v0.7.3
[0.7.2]: https://github.com/EmmanuelVinas/breezone/compare/v0.7.1...v0.7.2
[0.7.1]: https://github.com/EmmanuelVinas/breezone/compare/v0.7.0...v0.7.1
[0.7.0]: https://github.com/EmmanuelVinas/breezone/compare/v0.6.1...v0.7.0
[0.6.1]: https://github.com/EmmanuelVinas/breezone/compare/v0.6.0...v0.6.1
[0.6.0]: https://github.com/EmmanuelVinas/breezone/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/EmmanuelVinas/breezone/compare/v0.4.1...v0.5.0
[0.4.1]: https://github.com/EmmanuelVinas/breezone/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/EmmanuelVinas/breezone/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/EmmanuelVinas/breezone/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/EmmanuelVinas/breezone/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/EmmanuelVinas/breezone/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/EmmanuelVinas/breezone/releases/tag/v0.1.0
