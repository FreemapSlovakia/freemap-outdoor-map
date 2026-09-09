# Icon canvas review

`svg_repo::pad_viewport` grows every haloed icon's viewport by 1.5 px on each side
before librsvg sees it, so an icon no longer carries that padding in its own file.
`scripts/trim-icon-padding.py` removed it from the ones where that is provably a
no-op. This file is the other half: what it would not touch.

The first table is the one to read — those need a decision a script should not make.
Everything below it was skipped for a reason that needs no action.

Regenerate with `python3 scripts/trim-icon-padding.py` (add `--apply` to rewrite).

## Needs a look (5)

| icon | why it was skipped | geometry |
|---|---|---|
| `drinkable_spring.svg` | a layer of the composite spring icon: `spring_variant` merges these files into one document, so they share a coordinate system and can only be trimmed together, by a common offset | canvas 15×19, drawing 8.5,12.5 5×5 |
| `intermittent.svg` | a layer of the composite spring icon: `spring_variant` merges these files into one document, so they share a coordinate system and can only be trimmed together, by a common offset | canvas 15×19, drawing 0.5,2.5 14×15 |
| `mineral-spring.svg` | a layer of the composite spring icon: `spring_variant` merges these files into one document, so they share a coordinate system and can only be trimmed together, by a common offset | canvas 15×19, drawing 1.96467,5 11.7424×11 |
| `refitted_spring.svg` | a layer of the composite spring icon: `spring_variant` merges these files into one document, so they share a coordinate system and can only be trimmed together, by a common offset | canvas 15×19, drawing 1.49966,1.4997 11.0007×5.00064 |
| `spring.svg` | a layer of the composite spring icon: `spring_variant` merges these files into one document, so they share a coordinate system and can only be trimmed together, by a common offset | canvas 15×19, drawing 1.5,5.5 9×12 |

## Authored off the pixel grid (84)

Nothing to fix in the canvas, and nothing this script can do — listed because it
is the one thing that does still blur an icon.

`render_icons` snaps the ink box to the pixel grid, and the ink is the drawing
offset by a flat 1.5 px, so an icon's bounding box always lands on whole pixels —
whatever its size, odd, even or fractional. A straight horizontal or vertical edge
inside it is crisp exactly when it sits a whole number of pixels from that box. The
icons below have edges that do not, so those edges render soft at every zoom.
Curves are not counted: they are antialiased wherever they fall.

Moving the drawing does not help — the bounding box is measured from the drawing,
so a translate takes the reference with it. What matters is where each edge sits
*relative to the drawing's own extremes*. Where the fractions agree on an axis the
icon is internally consistent, and it is its outermost point — usually a curve —
that sits the odd half pixel away; moving the flat edges together by that fraction
(or adjusting the extreme) squares the lot up. Where they are mixed, the edges
disagree among themselves and only redrawing settles it.

| icon | flat edges off the grid | worst offset | edges agree with each other? |
|---|---|---|---|
| `archaeological_site.svg` | 8 of 12 | 0.500 px | vertical 0.5 px, horizontal 0 px |
| `synagogue.svg` | 12 of 21 | 0.500 px | vertical mixed, horizontal mixed |
| `bicycle.svg` | 6 of 13 | 0.500 px | vertical mixed, horizontal mixed |
| `water_tower.svg` | 4 of 10 | 0.500 px | vertical mixed, horizontal mixed |
| `alpine_hut.svg` | 4 of 11 | 0.500 px | vertical mixed, horizontal 0 px |
| `theatre.svg` | 3 of 10 | 0.500 px | vertical mixed, horizontal mixed |
| `tower_bell_tower.svg` | 3 of 10 | 0.500 px | vertical mixed, horizontal mixed |
| `biergarten.svg` | 3 of 17 | 0.500 px | vertical mixed, horizontal mixed |
| `phone.svg` | 6 of 38 | 0.500 px | vertical mixed, horizontal mixed |
| `wilderness_hut.svg` | 2 of 16 | 0.500 px | vertical mixed, horizontal 0 px |
| `caravan_site.svg` | 1 of 10 | 0.500 px | vertical mixed, horizontal 0 px |
| `townhall.svg` | 2 of 21 | 0.500 px | vertical mixed, horizontal 0 px |
| `tower_defensive.svg` | 2 of 22 | 0.500 px | vertical mixed, horizontal 0 px |
| `drinking_water.svg` | 1 of 14 | 0.500 px | vertical 0 px, horizontal mixed |
| `taxi.svg` | 1 of 18 | 0.500 px | vertical 0 px, horizontal mixed |
| `convenience.svg` | 18 of 45 | 0.500 px | vertical mixed, horizontal mixed |
| `supermarket.svg` | 9 of 31 | 0.500 px | vertical mixed, horizontal mixed |
| `shower.svg` | 6 of 6 | 0.500 px | vertical mixed, horizontal 0.5 px |
| `arts_centre.svg` | 4 of 5 | 0.500 px | vertical mixed, horizontal 0 px |
| `nightclub.svg` | 2 of 4 | 0.499 px | vertical mixed, horizontal 0 px |
| `toll_booth.svg` | 3 of 8 | 0.498 px | vertical 0 px, horizontal mixed |
| `audioguide.svg` | 7 of 14 | 0.496 px | vertical mixed, horizontal mixed |
| `bird_hide.svg` | 20 of 24 | 0.496 px | vertical mixed, horizontal mixed |
| `bus_station.svg` | 12 of 14 | 0.493 px | vertical mixed, horizontal mixed |
| `forester's_lodge.svg` | 4 of 4 | 0.490 px | vertical mixed, horizontal mixed |
| `chalet.svg` | 11 of 14 | 0.490 px | vertical mixed, horizontal mixed |
| `sauna.svg` | 8 of 9 | 0.481 px | vertical mixed, horizontal mixed |
| `tree_protected.svg` | 2 of 3 | 0.479 px | vertical 0.52 px, horizontal 0 px |
| `tree.svg` | 2 of 3 | 0.479 px | vertical 0.52 px, horizontal 0 px |
| `tower_communication.svg` | 3 of 16 | 0.476 px | vertical mixed, horizontal 0.52 px |
| `weir.svg` | 10 of 22 | 0.475 px | vertical mixed, horizontal mixed |
| `veterinary.svg` | 11 of 11 | 0.473 px | vertical 0.47 px, horizontal 0.88 px |
| `water_park.svg` | 1 of 1 | 0.460 px | vertical 0 px, horizontal 0.46 px |
| `station.svg` | 12 of 14 | 0.459 px | vertical mixed, horizontal mixed |
| `horse_riding.svg` | 11 of 15 | 0.455 px | vertical mixed, horizontal mixed |
| `university.svg` | 9 of 18 | 0.455 px | vertical mixed, horizontal mixed |
| `school.svg` | 7 of 16 | 0.451 px | vertical mixed, horizontal mixed |
| `lean_to.svg` | 9 of 11 | 0.449 px | vertical mixed, horizontal mixed |
| `information_office.svg` | 7 of 8 | 0.443 px | vertical mixed, horizontal mixed |
| `mosque.svg` | 8 of 14 | 0.443 px | vertical mixed, horizontal mixed |
| `basic_hut.svg` | 10 of 10 | 0.439 px | vertical mixed, horizontal mixed |
| `climbing.svg` | 9 of 11 | 0.412 px | vertical mixed, horizontal mixed |
| `soccer.svg` | 4 of 4 | 0.412 px | vertical mixed, horizontal 0.59 px |
| `ruins.svg` | 6 of 10 | 0.411 px | vertical mixed, horizontal mixed |
| `bureau_de_change.svg` | 5 of 25 | 0.402 px | vertical mixed, horizontal 0 px |
| `golf_course.svg` | 5 of 6 | 0.400 px | vertical mixed, horizontal mixed |
| `public_transport.svg` | 9 of 18 | 0.390 px | vertical mixed, horizontal mixed |
| `skiing.svg` | 4 of 4 | 0.372 px | vertical mixed, horizontal 0 px |
| `tower_observation.svg` | 3 of 20 | 0.344 px | vertical mixed, horizontal mixed |
| `information_terminal.svg` | 6 of 22 | 0.336 px | vertical mixed, horizontal mixed |
| `miniature_golf.svg` | 2 of 2 | 0.334 px | vertical mixed, horizontal 0 px |
| `charging_station.svg` | 8 of 20 | 0.333 px | vertical mixed, horizontal mixed |
| `picnic_shelter.svg` | 16 of 16 | 0.331 px | vertical mixed, horizontal 0.97 px |
| `shelter.svg` | 4 of 9 | 0.314 px | vertical 0.31 px, horizontal 0 px |
| `map.svg` | 2 of 14 | 0.270 px | vertical mixed, horizontal 0 px |
| `kindergarten.svg` | 7 of 8 | 0.260 px | vertical mixed, horizontal mixed |
| `fountain.svg` | 7 of 8 | 0.258 px | vertical mixed, horizontal 0.98 px |
| `bicycle_repair_station.svg` | 3 of 19 | 0.256 px | vertical mixed, horizontal mixed |
| `public_bath.svg` | 7 of 9 | 0.250 px | vertical mixed, horizontal 0.75 px |
| `fast_food.svg` | 5 of 10 | 0.250 px | vertical 0.75 px, horizontal mixed |
| `lighthouse.svg` | 4 of 9 | 0.250 px | vertical 0 px, horizontal mixed |
| `toilets.svg` | 6 of 14 | 0.250 px | vertical mixed, horizontal 0 px |
| `car_rental.svg` | 3 of 11 | 0.250 px | vertical 0 px, horizontal mixed |
| `bicycle_rental.svg` | 4 of 17 | 0.250 px | vertical 0 px, horizontal mixed |
| `cafe.svg` | 2 of 11 | 0.250 px | vertical 0 px, horizontal mixed |
| `courthouse.svg` | 2 of 11 | 0.250 px | vertical mixed, horizontal 0 px |
| `volleyball.svg` | 1 of 1 | 0.191 px | vertical 0.81 px, horizontal 0 px |
| `manger.svg` | 2 of 2 | 0.186 px | vertical 0.19 px, horizontal 0 px |
| `basketball.svg` | 14 of 15 | 0.163 px | vertical mixed, horizontal mixed |
| `picnic_site.svg` | 2 of 17 | 0.130 px | vertical mixed, horizontal 0 px |
| `mast.svg` | 4 of 7 | 0.113 px | vertical 0.89 px, horizontal 0 px |
| `gallery.svg` | 1 of 1 | 0.111 px | vertical 0 px, horizontal 0.11 px |
| `casino.svg` | 6 of 6 | 0.101 px | vertical 0.9 px, horizontal 0.91 px |
| `free_flying.svg` | 1 of 1 | 0.093 px | vertical 0 px, horizontal 0.91 px |
| `storage_tank.svg` | 1 of 11 | 0.059 px | vertical 0 px, horizontal mixed |
| `church.svg` | 1 of 24 | 0.028 px | vertical mixed, horizontal mixed |
| `college.svg` | 2 of 27 | 0.021 px | vertical mixed, horizontal 0 px |

88 icons are fully on the grid, and 18 have no straight axis-aligned edge to judge (`attraction`, `beach_resort`, `bollard`, `dentist`, `disused_mine`, `drinkable_spring`, `fire_station`, `firepit`, `greengrocer`, `intermittent`, `mine`, `obstacle_tree`, `obstacle_vegetation`, `refitted_spring`, `sinkhole`, `stone`, `tennis`, `viewpoint`).

## Skipped, nothing to decide (218)

**Already tight** — 183

> `aerodrome`, `alpine_hut`, `apartment`, `arch`, `archaeological_site`, `arts_centre`, `artwork`, `atm`, `attraction`, `audioguide`, `bank`, `bar`, `basic_hut`, `basketball`, `bbq`, `beach_resort`, `beehive`, `bench`, `bicycle`, `bicycle_rental`, `bicycle_repair_station`, `biergarten`, `bird_hide`, `board`, `bollard`, `boundary_stone`, `bowling_alley`, `building`, `bunker`, `bureau_de_change`, `bus_station`, `bus_stop`, `cafe`, `camp_site`, `car_rental`, `caravan_site`, `casino`, `castle`, `cattle_grid`, `cave_entrance`, `chalet`, `chapel`, `charging_station`, `chimney`, `church`, `cinema`, `city_gate`, `climbing`, `college`, `community_centre`, `confectionery`, `convenience`, `courthouse`, `cross`, `cycle_barrier`, `cycling`, `dam`, `dance`, `dentist`, `disused_mine`, `doctors`, `drinking_water`, `fast_food`, `ferry_terminal`, `fire_station`, `firepit`, `fishing`, `fitness_centre`, `fitness_station`, `ford`, `forester's_lodge`, `fountain`, `free_flying`, `fuel`, `full-height_turnstile`, `gallery`, `gate`, `generator_wind`, `golf_course`, `greengrocer`, `guest_house`, `guidepost_x`, `guidepost_xx`, `helipad`, `horse_riding`, `hospital`, `hostel`, `hotel`, `hunting_stand`, `ice_cream`, `ice_skating`, `information_office`, `information_terminal`, `kindergarten`, `kissing_gate`, `lean_to`, `lift_gate`, `lighthouse`, `manger`, `manor`, `map`, `marketplace`, `massage`, `mast`, `memorial`, `mine`, `miniature_golf`, `monument`, `mosque`, `motel`, `motorcycle_barrier`, `museum`, `nightclub`, `obelisk`, `obstacle`, `obstacle_tree`, `obstacle_vegetation`, `outdoor_seating`, `parcel_locker`, `parking`, `pharmacy`, `phone`, `picnic_shelter`, `picnic_site`, `picnic_table`, `playground`, `police`, `post_box`, `post_office`, `prison`, `pub`, `public_bath`, `public_transport`, `restaurant`, `rock`, `ruins`, `running`, `sauna`, `school`, `shelter`, `shooting`, `shower`, `sinkhole`, `skiing`, `soccer`, `station`, `stile`, `stone`, `storage_tank`, `supermarket`, `synagogue`, `taxi`, `telephone`, `tennis`, `theatre`, `toilets`, `toll_booth`, `tower`, `tower_bell_tower`, `tower_communication`, `tower_cooling`, `tower_defensive`, `tower_observation`, `townhall`, `tree`, `tree_protected`, `university`, `veterinary`, `viewpoint`, `volleyball`, `waste_basket`, `waste_disposal`, `water_park`, `water_tower`, `water_well`, `water_works`, `waterfall`, `watering_place`, `wayside_shrine`, `weather_shelter`, `weir`, `wilderness_hut`, `windmill`

**Used as a pattern tile or line decoration, where the canvas is the tile** — 33

> `bare_rock`, `bog`, `clearcut2`, `cliff`, `dog_park`, `dyke`, `earth_bank`, `embankment`, `embankment-half`, `fixme`, `glacier`, `grapes`, `grave`, `gully`, `highway-arrow`, `horse`, `mangrove`, `marsh`, `no_bicycle`, `no_foot`, `orchard`, `plant_nursery`, `protected_area`, `quarry`, `reedbed`, `sand`, `scree`, `scrub`, `ski`, `swamp`, `tree2`, `waterway-arrow`, `wetland`

**Drawn with `halo: false`, so the renderer adds no padding** — 2

> `peak`, `saddle`
