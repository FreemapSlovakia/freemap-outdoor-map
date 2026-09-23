#!/usr/bin/env nu

# Write <base>/<key>/attribution.json for every hillshading dataset present, so
# GET /licenses can resolve the `shading:<key>` and `contours:<key>` codes tiles
# carry. Skips keys with no final.tif, and is safe to re-run.
#
#   nu scripts/write-dtm-attribution.nu /home/martin/14TB/hillshading
#
# `covers` lists both namespaces because every contour set here is contoured from
# the same national DEM the shading comes from — contours-de-sn.nu contours what
# shading-de-sn.nu emitted, and so on. A region that ever takes its contours from
# a different provider drops "contours" here and gets its own entry instead.
#
# The canonical table is the elevation API's, in
# /fm/storage1/backend.freemap.sk-data/elevation-sources/*/source.json on fm6 (a git
# repo) — the same sources feed both, so re-sync from there rather than retyping, and
# add a dataset there first. The webapp's OUTDOOR_NATIONAL_DTM_ATTRIBUTION is a third
# copy of the same list, until it starts reading /licenses instead.

def sources [] {
  [
    {
      key: "_"
      sources: [
        {
          title: "GEDTM30"
          url: "https://codeberg.org/openlandmap/GEDTM30"
        }
      ]
    }
    {
      key: "at"
      sources: [
        {
          title: "ALS DTM: Digitales Geländemodell Österreich (Geoland.at open data)"
          url: "https://www.data.gv.at/katalog/dataset/d88a1246-9684-480b-a480-ff63286b35b7"
        }
      ]
    }
    {
      key: "be"
      sources: [
        {
          title: "MNT 1 m 2021–2022: © Service public de Wallonie (SPW), CC BY 4.0 — modified"
          url: "https://geoportail.wallonie.be/catalogue/fe13bc84-e371-46ca-9632-8ad4139f1ee5.html"
        }
        {
          title: "DHMV II 1 m — Bron: Digitaal Vlaanderen (Modellicentie gratis hergebruik)"
          url: "https://metadata.vlaanderen.be/srv/dut/catalog.search#/metadata/f52b1a13-86bc-4b64-8256-88cc0d1a8735"
        }
      ]
    }
    {
      key: "ch"
      sources: [
        {
          title: "swissALTI3D: © swisstopo"
          url: "https://www.swisstopo.admin.ch/en/height-models/swissalti3d.html"
        }
      ]
    }
    {
      key: "cz"
      sources: [
        {
          title: "DMR 5G: ČÚZK Geoportál"
          url: "https://geoportal.cuzk.cz/(S(a21rqp1jhcnkz4iqcen2w50l))/Default.aspx?head_tab=sekce-02-gp&lng=EN&menu=302&metadataID=CZ-CUZK-DMR5G-V&mode=TextMeta&side=vyskopis"
        }
      ]
    }
    {
      key: "de_by"
      sources: [
        {
          title: "Bayerische Vermessungsverwaltung: DGM1 1 m, dl-de/by-2-0"
          url: "https://geodaten.bayern.de/opengeodata/OpenDataDetail.html?pn=dgm1"
        }
      ]
    }
    {
      key: "de_ni"
      sources: [
        {
          title: "Landesamt für Geoinformation und Landesvermessung Niedersachsen (LGLN): DGM1 1 m, CC BY 4.0"
          url: "https://ni-lgln-opengeodata.hub.arcgis.com/items/18cc3ed281db489e9b4d47c835f14ea9"
        }
      ]
    }
    {
      key: "de_nw"
      sources: [
        {
          title: "Land Nordrhein-Westfalen / Geobasis NRW (2022): DGM1 1 m, dl-de/zero-2-0"
          url: "https://open.nrw/dataset/0c6796e5-9eca-4ae6-8b32-1fcc5ae5c481"
        }
      ]
    }
    {
      key: "de_sn"
      sources: [
        {
          title: "GeoSN (Staatsbetrieb Geobasisinformation und Vermessung Sachsen): DGM1 1 m, dl-de/by-2-0"
          url: "https://www.geodaten.sachsen.de/digitale-hoehenmodelle-3994.html"
        }
      ]
    }
    {
      key: "de_th"
      sources: [
        {
          title: "GDI-Th, Freistaat Thüringen: DGM1 1 m, dl-de/by-2-0"
          url: "https://www.geoportal-th.de/de-de/Downloadbereiche/Download-Offene-Geodaten-Th%C3%BCringen/Download-H%C3%B6hendaten"
        }
      ]
    }
    {
      key: "en"
      sources: [
        {
          title: "LIDAR Composite DTM 1 m (England, OGL v3): © Environment Agency copyright and/or database right 2022. All rights reserved."
          url: "https://www.data.gov.uk/dataset/01b3ee39-da3f-47b6-83da-dc98e73a461f/lidar-composite-digital-terrain-model-dtm-1m"
        }
      ]
    }
    {
      key: "es"
      sources: [
        {
          title: "MDT05: IGN (CNIG)"
          url: "https://centrodedescargas.cnig.es/CentroDescargas/modelos-digitales-elevaciones"
        }
      ]
    }
    {
      key: "fi"
      sources: [
        {
          title: "Korkeusmalli 2 m: Maanmittauslaitos"
          url: "https://www.maanmittauslaitos.fi/en/maps-and-spatial-data/datasets-and-interfaces/product-descriptions/elevation-model-2-m"
        }
      ]
    }
    {
      key: "fr"
      sources: [
        {
          title: "RGE ALTI: IGN (Etalab Open Licence)"
          url: "https://geoservices.ign.fr/rgealti"
        }
      ]
    }
    {
      key: "hr"
      sources: [
        {
          title: "DMR: Državna geodetska uprava"
          url: "https://dgu.gov.hr/proizvodi-i-usluge/podaci-topografske-izmjere/digitalni-model-reljefa/180"
        }
      ]
    }
    {
      key: "it"
      sources: [
        {
          title: "HR-DTM 5 m: IRPI-CNR"
          url: "https://doi.org/10.5281/zenodo.18335145"
        }
      ]
    }
    {
      key: "lu"
      sources: [
        {
          title: "MNT LiDAR 2024: Administration du cadastre et de la topographie (CC0)"
          url: "https://data.public.lu/en/datasets/lidar-2024-releve-3d-du-territoire-luxembourgeois/"
        }
      ]
    }
    {
      key: "nl"
      sources: [
        {
          title: "Actueel Hoogtebestand Nederland (AHN5): DTM 1 m, CC0"
          url: "https://www.ahn.nl/"
        }
      ]
    }
    {
      key: "no"
      sources: [
        {
          title: "DTM: Kartverket (NLOD 2.0)"
          url: "https://hoydedata.no/"
        }
      ]
    }
    {
      key: "pl"
      sources: [
        {
          title: "NMT: GUGiK"
          url: "https://www.geoportal.gov.pl/"
        }
      ]
    }
    {
      key: "se"
      sources: [
        {
          title: "Markhöjdmodell Nedladdning: Lantmäteriet"
          url: "https://www.lantmateriet.se/en/geodata/our-products/product-list/elevation-model-download/"
        }
      ]
    }
    {
      key: "si"
      sources: [
        {
          title: "DMR: Ministrstvo za okolje in prostor"
          url: "https://gis.arso.gov.si/evode/profile.aspx?id=atlas_voda_Lidar@Arso"
        }
      ]
    }
    {
      key: "sk"
      sources: [
        {
          title: "DMR 5.0: ÚGKK SR"
          url: "https://www.skgeodesy.sk/gku/produkty-sluzby/na-stiahnutie/zbgis.html#lls"
        }
      ]
    }
  ]
}

def main [
  base: path  # hillshading base directory, the one MAPRENDER_HILLSHADING_BASE_PATH points at
] {
  if not ($base | path exists) {
    error make { msg: $"no such directory: ($base)" }
  }

  let known = (sources | get key)

  for entry in (sources) {
    let dir = ($base | path join $entry.key)

    if not (($dir | path join "final.tif") | path exists) {
      continue
    }

    let out = ($dir | path join "attribution.json")

    {
      covers: ["shading", "contours"]
      sources: $entry.sources
    } | to json --indent 2 | save --force $out

    print $"wrote ($out)"
  }

  # A dataset with no row above would silently serve an unresolvable code.
  let missing = (
    ls $base
    | where type == dir
    | get name
    | path basename
    | where {|key| (($base | path join $key "final.tif") | path exists) and ($key not-in $known) }
  )

  if ($missing | is-not-empty) {
    print $"no attribution defined for: ($missing | str join ', ')"
  }
}
