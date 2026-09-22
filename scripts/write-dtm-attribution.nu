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
# The titles are the ones the webapp shows, from OUTDOOR_NATIONAL_DTM_ATTRIBUTION
# in freemap-v3-react's src/shared/mapDefinitions.tsx. They are duplicated here
# only until the webapp reads them back from /licenses; keep the two in step, and
# add a row below when a dataset is added.

def sources [] {
  [
    {
      key: "_"
      sources: [{ title: "GEDTM30", url: "https://codeberg.org/openlandmap/GEDTM30" }]
    }
    {
      key: "at"
      sources: [{
        title: "ALS DTM: Digitales Geländemodell Österreich (Geoland.at open data)"
        url: "https://www.data.gv.at/katalog/dataset/d88a1246-9684-480b-a480-ff63286b35b7"
      }]
    }
    {
      key: "be"
      sources: [
        {
          title: "MNT 1 m 2021–2022: © Service public de Wallonie (SPW), CC BY 4.0 — modified"
          url: "https://geoportail.wallonie.be/catalogue/fe13bc84-e371-46ca-9632-8ad4139f1ee5.html"
        }
        {
          title: "DHMV II 1 m — Bron: Digitaal Vlaanderen (Modellicentie gratis hergebruik)"
          url: "https://metadata.vlaanderen.be/srv/dut/catalog.search#/metadata/f52b1a13-86bc-4b64-8256-88cc0d1a8735"
        }
      ]
    }
    {
      key: "ch"
      sources: [{
        title: "swissALTI3D: © swisstopo"
        url: "https://www.swisstopo.admin.ch/en/height-models/swissalti3d.html"
      }]
    }
    {
      key: "cz"
      sources: [{
        title: "DMR 5G: ČÚZK Geoportál"
        url: "https://geoportal.cuzk.cz/Default.aspx?head_tab=sekce-02-gp&lng=EN&menu=302&metadataID=CZ-CUZK-DMR5G-V&mode=TextMeta&side=vyskopis"
      }]
    }
    # The German states and the Netherlands are not in the webapp's table; their
    # titles come from the header of the script that downloaded each DEM, which is
    # where the licence was recorded at the time. de_ni and de_th quote the
    # Namensnennung those licences ask for verbatim.
    {
      key: "de_by"
      sources: [{
        title: "DGM1: Bayerische Vermessungsverwaltung – www.geodaten.bayern.de (dl-de/by-2-0)"
      }]
    }
    {
      key: "de_ni"
      sources: [{ title: "DGM1: LGLN (CC BY 4.0)" }]
    }
    {
      # dl-de/zero-2-0 requires no attribution; credited anyway.
      key: "de_nw"
      sources: [{ title: "DGM1: Land NRW – geobasis.nrw.de (dl-de/zero-2-0)" }]
    }
    {
      key: "de_sn"
      sources: [{
        title: "DGM1: GeoSN (dl-de/by-2-0)"
        url: "https://www.geodaten.sachsen.de/batch-download-4719.html"
      }]
    }
    {
      key: "de_th"
      sources: [{ title: "DGM1: GDI-Th, Freistaat Thüringen (dl-de/by-2-0)" }]
    }
    {
      # AHN5 via PDOK. download-nl.nu records no licence, so none is claimed here.
      key: "nl"
      sources: [{ title: "AHN5 DTM 0,5 m: PDOK (Rijkswaterstaat)" }]
    }
    {
      key: "en"
      sources: [{
        title: "LIDAR Composite DTM 1 m (England, OGL v3): © Environment Agency copyright and/or database right 2022. All rights reserved."
        url: "https://www.data.gov.uk/dataset/01b3ee39-da3f-47b6-83da-dc98e73a461f/lidar-composite-digital-terrain-model-dtm-1m"
      }]
    }
    {
      key: "es"
      sources: [{
        title: "MDT05: IGN (CNIG)"
        url: "https://centrodedescargas.cnig.es/CentroDescargas/modelos-digitales-elevaciones"
      }]
    }
    {
      key: "fi"
      sources: [{
        title: "Korkeusmalli 2 m: Maanmittauslaitos"
        url: "https://www.maanmittauslaitos.fi/en/maps-and-spatial-data/datasets-and-interfaces/product-descriptions/elevation-model-2-m"
      }]
    }
    {
      key: "fr"
      sources: [{
        title: "RGE ALTI: IGN (Etalab Open Licence)"
        url: "https://geoservices.ign.fr/rgealti"
      }]
    }
    {
      key: "hr"
      sources: [{
        title: "DMR: Državna geodetska uprava"
        url: "https://dgu.gov.hr/proizvodi-i-usluge/podaci-topografske-izmjere/digitalni-model-reljefa/180"
      }]
    }
    {
      key: "it"
      sources: [{
        title: "HR-DTM 5 m: IRPI-CNR"
        url: "https://doi.org/10.5281/zenodo.18335145"
      }]
    }
    {
      key: "lu"
      sources: [{
        title: "MNT LiDAR 2024: Administration du cadastre et de la topographie (CC0)"
        url: "https://data.public.lu/en/datasets/lidar-2024-releve-3d-du-territoire-luxembourgeois/"
      }]
    }
    {
      key: "no"
      sources: [{
        title: "DTM: Kartverket (NLOD 2.0)"
        url: "https://hoydedata.no/"
      }]
    }
    {
      key: "pl"
      sources: [{ title: "NMT: GUGiK", url: "https://www.geoportal.gov.pl/" }]
    }
    {
      key: "se"
      sources: [{
        title: "Markhöjdmodell Nedladdning: Lantmäteriet"
        url: "https://www.lantmateriet.se/en/geodata/our-products/product-list/elevation-model-download/"
      }]
    }
    {
      key: "si"
      sources: [{
        title: "DMR: Ministrstvo za okolje in prostor"
        url: "https://gis.arso.gov.si/evode/profile.aspx?id=atlas_voda_Lidar@Arso"
      }]
    }
    {
      key: "sk"
      sources: [{
        title: "DMR 5.0: ÚGKK SR"
        url: "https://www.skgeodesy.sk/gku/produkty-sluzby/na-stiahnutie/zbgis.html#lls"
      }]
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
