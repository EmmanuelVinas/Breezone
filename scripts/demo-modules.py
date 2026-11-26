#!/usr/bin/env python3
"""Adds the fake modules of mock-airzone (MOCK_DEVICES_PORT) to a Breezone server, for demos and
screenshots: one module of each kind, its devices named, placed in rooms, the energy measures
given their role. Run it once, on a development server only:

    ./scripts/dev.sh --modules        # does it for you
    python3 scripts/demo-modules.py http://localhost:8427 admin motdepasse1 [first port, 3100] [database]

With the database (data/dev/breezone.db), it also writes 30 days of history of the modules.
"""

import json
import sys
import time
import urllib.error
import urllib.request

base, user, password = sys.argv[1], sys.argv[2], sys.argv[3]
first = int(sys.argv[4]) if len(sys.argv) > 4 else 3100
token = None


def call(method, path, body=None):
    req = urllib.request.Request(base + path, method=method, data=None if body is None else json.dumps(body).encode())
    req.add_header("Content-Type", "application/json")
    if token:
        req.add_header("Authorization", f"Bearer {token}")
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            text = r.read().decode()
            return json.loads(text) if text else None
    except urllib.error.HTTPError as e:
        print(f"  {method} {path}: HTTP {e.code} {e.read().decode()[:200]}")
        return None


token = call("POST", "/api/auth/login", {"username": user, "password": password})["access_token"]
# The demonstration speaks French, as the screenshots of the README.
call("PUT", "/api/me/language", {"language": "fr"})
host = lambda n: f"127.0.0.1:{first + n}"

# A fresh database: the home of the demonstration, on the fake WebServer, in a city that is
# nobody's (the screenshots must not tell where one lives).
if not call("GET", "/api/installations"):
    print("▸ Maison (Airzone)")
    call("POST", "/api/installations", {"name": "Maison", "host": "127.0.0.1", "port": 3000, "location": "Annecy, Haute-Savoie"})

# The city is placed on the map in the background: the forecast of the solar production needs it.
call("POST", "/api/accounts/weather/test")
for _ in range(30):
    if (call("GET", "/api/weather") or {}).get("weather"):
        break
    time.sleep(1)

sources = call("GET", "/api/sources") or []
existing = {s["driver"] for s in sources}
named = {(s["driver"], s["name"]) for s in sources}
modules = [
    ("shelly", "Tableau électrique", {"host": host(0)}),
    ("trydan", "Borne de recharge", {"host": host(2)}),
    ("renson", "VMC", {"host": host(3)}),
    ("tasmota", "Salon", {"host": host(4)}),
    ("esphome", "Volets et jardin", {"host": host(5)}),
    ("tasmota", "Pont Zigbee", {"host": host(7)}),
]
for driver, name, config in modules:
    if (driver, name) in named:
        continue
    print(f"▸ {name} ({driver})")
    call("POST", "/api/sources", {"driver": driver, "name": name, "config": config})
if "forecast_solar" not in existing:
    # A service (Settings › Accounts and services): the server asks the fake one
    # (BREEZONE_FORECAST_SOLAR_URL, set by dev.sh --modules).
    print("▸ Prévision solaire (forecast_solar)")
    call("PUT", "/api/accounts/forecast_solar", {"kwp": "4,5", "declination": "30", "azimuth": "0"})
if "hue" not in existing:
    print("▸ Pont Hue (hue)")
    call("POST", "/api/modules/hue/pairing", {"step": "link", "input": {"host": host(1)}, "name": "Pont Hue"})

rooms = {r["name"]: r["id"] for r in call("GET", "/api/rooms") or []}
for name in ["Salon", "Chambre", "Cuisine", "Salle de bains", "Bureau", "Dressing", "Garage", "Jardin"]:
    if name not in rooms:
        r = call("POST", "/api/rooms", {"name": name, "position": len(rooms)})
        if r:
            rooms[name] = r["id"]

# (source, reference) → name, room, role
plan = {
    ("shelly", "em1:0"): ("Réseau électrique", "Garage", "grid"),
    ("shelly", "em1:1"): ("Panneaux solaires", "Jardin", "solar"),
    ("shelly", "switch:0"): ("Chauffe-eau", "Garage", None),
    ("shelly", "switch:1"): ("Prise du bureau", "Bureau", None),
    ("trydan", None): ("Borne de recharge", "Garage", None),
    ("renson", "unit"): (None, "Garage", None),
    ("renson", "room:3"): (None, "Salle de bains", None),
    ("forecast_solar", None): ("Prévision solaire", "Jardin", None),
    ("tasmota", "power1"): ("Lampadaire", "Salon", None),
    ("tasmota", "power2"): ("Télévision", "Salon", None),
    ("tasmota", "energy"): ("Télévision · mesure", "Salon", None),
    ("tasmota", "sensor/BME280"): ("Capteur du salon", "Salon", None),
    ("tasmota", "zigbee/0x00124B0026B64D1E"): (None, "Chambre", None),
    ("tasmota", "zigbee/0x7CB03EBB0A0292DD"): (None, "Cuisine", None),
    ("tasmota", "zigbee/0x0017880102AABBCC"): (None, "Salon", None),
    ("esphome", "cover/volet_salon"): (None, "Salon", None),
    ("esphome", "cover/volet_chambre"): (None, "Chambre", None),
    ("esphome", "cover/volet_bureau"): (None, "Bureau", None),
    ("esphome", "cover/volet_cuisine"): (None, "Cuisine", None),
    ("esphome", "switch/arrosage"): (None, "Jardin", None),
    ("esphome", "air"): ("Air du bureau", "Bureau", None),
    ("hue", "1"): (None, "Salon", None),
    ("hue", "2"): (None, "Salon", None),
    ("hue", "3"): (None, "Chambre", None),
}
renson_rooms = {"Salle de bains": "Salle de bains", "Cuisine": "Cuisine"}

for d in call("GET", "/api/devices") or []:
    if d["source"] == "airzone":
        # The zones go in the room of their name ("Chambre 1" in « Chambre »).
        room = next((r for r in rooms if d["name"].startswith(r)), None)
        if room and d.get("room_id") is None:
            call("PUT", f"/api/devices/{d['id']}", {"room_id": rooms[room]})
        continue
    if d.get("source_id") is None:
        continue
    ref = d["reference"] if isinstance(d["reference"], str) else json.dumps(d["reference"])
    entry = plan.get((d["source"], ref)) or plan.get((d["source"], None))
    if d["source"] == "renson" and d["name"] in renson_rooms:
        entry = (None, renson_rooms[d["name"]], None)
    if d["source"] == "hue":
        entry = plan.get(("hue", ref.split(":")[-1]))
    if not entry:
        continue
    name, room, role = entry
    body = {}
    if name:
        body["name"] = name
    if room in rooms:
        body["room_id"] = rooms[room]
    if role:
        body["role"] = role
    if body:
        call("PUT", f"/api/devices/{d['id']}", body)
print("✓ Modules de démonstration ajoutés")

# The history of the last 30 days, written straight into the database (a development one only),
# so that the curves and the energy page have something to show.
if len(sys.argv) > 5:
    import math
    import random
    import sqlite3
    import time

    def solar(h, day):
        if not 7.5 <= h <= 18.5:
            return 0.0
        bell = math.sin((h - 7.5) / 11.0 * math.pi)
        clouds = 1.0 - 0.5 * random.Random(day).random() * max(0.0, math.sin(h * 2.7))
        return round(4200 * bell * bell * clouds)

    def house(h):
        return 320 + 900 * math.exp(-((h - 7.5) ** 2) / 1.5) + 1500 * math.exp(-((h - 20) ** 2) / 3) + 60 * math.sin(h * 5)

    heater = lambda h: 1800.0 if 12 <= h < 15 or 2 <= h < 4 else 0.0
    car = lambda h, day: 3700.0 if 9.5 <= h < 15.5 and day % 4 != 1 else 0.0

    ids = {(d["source"], d["reference"] if isinstance(d["reference"], str) else ""): d["id"] for d in call("GET", "/api/devices") or []}
    fields = {
        ("shelly", "em1:0"): lambda h, day: {"power": house(h) + heater(h) + car(h, day) - solar(h, day)},
        ("shelly", "em1:1"): lambda h, day: {"power": -solar(h, day)},
        ("shelly", "switch:0"): lambda h, day: {"power": heater(h), "on": 1.0 if heater(h) else 0.0},
        ("trydan", "charger"): lambda h, day: {"power": car(h, day)},
        ("tasmota", "energy"): lambda h, day: {"power": 85.0 if 19 <= h < 23 else 0.0},
        ("tasmota", "sensor/BME280"): lambda h, day: {"temperature": round(21.2 + 0.8 * math.cos((h - 15) / 24 * 2 * math.pi), 1), "humidity": 47.0},
        ("tasmota", "zigbee/0x00124B0026B64D1E"): lambda h, day: {"temperature": round(19.4 + 0.6 * math.cos((h - 16) / 24 * 2 * math.pi), 1), "humidity": round(55 - 4 * math.cos((h - 16) / 24 * 2 * math.pi), 1)},
        ("renson", "room:1"): lambda h, day: {"humidity": round(55 + 25 * math.exp(-((h - 7.3) ** 2) / 0.3), 1), "flow": 40.0 if 6.8 < h < 8 else 18.0},
        ("renson", "room:2"): lambda h, day: {"humidity": round(52 + 6 * math.exp(-((h - 19.5) ** 2) / 0.5), 1), "co2": round(640 + 120 * math.sin(h * 0.8)), "flow": 32.0},
        ("renson", "room:3"): lambda h, day: {"humidity": 49.0, "flow": 10.0},
        ("esphome", "air"): lambda h, day: {"co2": round(620 + 180 * math.sin(h * 0.9)), "temperature": round(20.6 + 0.5 * math.sin(h / 3), 1), "humidity": round(51 + 4 * math.cos(h / 2), 1)},
    }
    db = sqlite3.connect(sys.argv[5], timeout=30)
    now = int(time.time()) // 300 * 300
    rows = []
    for ts in range(now - 30 * 86400, now - 300, 300):
        t = time.localtime(ts)
        h = t.tm_hour + t.tm_min / 60
        for key, f in fields.items():
            if key in ids:
                rows += [(ids[key], ts, field, float(v)) for field, v in f(h, t.tm_yday).items()]
    db.executemany("INSERT OR REPLACE INTO device_samples (device_id, ts, field, value) VALUES (?, ?, ?, ?)", rows)
    db.commit()
    print(f"✓ {len(rows)} points d'historique sur 30 jours")
