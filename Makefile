INPUT ?= dtm.vrt
ZOOM  ?= 16

TR    := $(shell echo "scale=20; 4*a(1)*2*6378137/256/(2^$(ZOOM))" | bc -l)
$(info ZOOM=$(ZOOM) TR=$(TR))

MAKEFLAGS += -j4

.DEFAULT_GOAL := build/final.tif.ovr
.PHONY: clean build/final.tif.ovr

build:
	mkdir -p build

clean:
	@read -p "Delete build/ directory? [y/N] " ans && [ "$$ans" = y ]
	rm -rf build

build/final.tif.ovr: build/final.tif
	GDAL_CACHEMAX=4096 gdaladdo --config GDAL_TIFF_INTERNAL_MASK YES -r cubic build/final.tif 2 4 8 16 32 64 128 256

define gen_relief
  gdaldem hillshade $(INPUT) build/_$(1).tif -az $(2) -igor -compute_edges -co COMPRESS=ZSTD -co PREDICTOR=2 -co TILED=YES -co NUM_THREADS=ALL_CPUS
endef

build/_a.tif: $(INPUT) | build
	$(call gen_relief,a,-120)

build/_b.tif: $(INPUT) | build
	$(call gen_relief,b,60)

build/_c.tif: $(INPUT) | build
	$(call gen_relief,c,-45)

define gen_warp
  gdalwarp -t_srs 'EPSG:3857' -tr $(TR) $(TR) -tap -r cubic -of GTiff -co COMPRESS=ZSTD -co BIGTIFF=YES -co TILED=YES -multi -wo NUM_THREADS=ALL_CPUS -co NUM_THREADS=ALL_CPUS build/_$(1).tif build/$(1)-warped.tif
endef

build/a-warped.tif: build/_a.tif
	$(call gen_warp,a)

build/b-warped.tif: build/_b.tif
	$(call gen_warp,b)

build/c-warped.tif: build/_c.tif
	$(call gen_warp,c)

a := 0.8 * (255 - A)
b := 0.7 * (255 - B)
c := 1.0 * (255 - C)

contrast := 1.0
brightness := 0.0

define gen_band
	gdal_calc.py --co=BIGTIFF=YES --co=TILED=YES --co=NUM_THREADS=ALL_CPUS --co=COMPRESS=ZSTD --co=PREDICTOR=2 -A build/a-warped.tif -B build/b-warped.tif -C build/c-warped.tif --outfile=build/$(1).tif \
		--calc="$(contrast) * (($(a) * $(2) + $(b) * $(3) + $(c) * $(4)) / (0.01 + $(a) + $(b) + $(c)) - 128.0) + 128.0 + $(brightness)"
endef

# RGB colors per sub-relief are defined in columns
#          [a]  [b]  [c]

build/R.tif: build/a-warped.tif build/b-warped.tif build/c-warped.tif
	$(call gen_band,R,0x20,0xFF,0x00)

build/G.tif: build/a-warped.tif build/b-warped.tif build/c-warped.tif
	$(call gen_band,G,0x30,0xEE,0x00)

build/B.tif: build/a-warped.tif build/b-warped.tif build/c-warped.tif
	$(call gen_band,B,0x60,0x00,0x00)

build/A.tif: build/a-warped.tif build/b-warped.tif build/c-warped.tif
	gdal_calc.py --co=BIGTIFF=YES --co=TILED=YES --co=NUM_THREADS=ALL_CPUS --co=COMPRESS=ZSTD --co=PREDICTOR=2 --NoDataValue=0 -A build/a-warped.tif -B build/b-warped.tif -C build/c-warped.tif --outfile=build/A.tif \
		--calc="255.0 - 255.0 * ((1.0 - $(a) / 255.0) * (1.0 - $(b) / 255.0) * (1.0 - $(c) / 255.0))"

build/stack-with-mask.vrt: build/R.tif build/G.tif build/B.tif build/A.tif build/a-warped.tif
	gdalbuildvrt -separate build/stack-with-mask.vrt build/R.tif build/G.tif build/B.tif build/A.tif
	gdal_edit.py -colorinterp_1 red -colorinterp_2 green -colorinterp_3 blue -colorinterp_4 alpha build/stack-with-mask.vrt
	sed -i '/<NoDataValue>/d; /<NODATA>/d; /<SrcRect/d; /<DstRect/d; s/ComplexSource/SimpleSource/g' build/stack-with-mask.vrt
	sed -i 's|</VRTDataset>|<MaskBand><VRTRasterBand dataType="Byte"><SimpleSource><SourceFilename relativeToVRT="1">a-warped.tif</SourceFilename><SourceBand>1</SourceBand></SimpleSource></VRTRasterBand></MaskBand></VRTDataset>|' build/stack-with-mask.vrt

build/final.tif: build/stack-with-mask.vrt
	gdal_translate --config GDAL_TIFF_INTERNAL_MASK YES -of GTiff -co TILED=YES -co COMPRESS=ZSTD -co PREDICTOR=2 -co BIGTIFF=YES -co NUM_THREADS=ALL_CPUS build/stack-with-mask.vrt build/final.tif


