#!/usr/bin/env python3
"""Capture GD 2.3.3 pixel behavior using synthetic images, without image parsing."""
import argparse
import ctypes
import ctypes.util
import json
import pathlib
lib = ctypes.CDLL(ctypes.util.find_library('gd'))
def function(name, result, *args):
    fn = getattr(lib, name)
    fn.restype, fn.argtypes = result, list(args)
    return fn
ptr, integer = ctypes.c_void_p, ctypes.c_int
version = function('gdVersionString', ctypes.c_char_p)
create = function('gdImageCreateTrueColor', ptr, integer, integer)
blend = function('gdImageAlphaBlending', None, ptr, integer)
set_pixel = function('gdImageSetPixel', None, ptr, integer, integer, integer)
get_pixel = function('gdImageGetTrueColorPixel', integer, ptr, integer, integer)
resize = function('gdImageCopyResampled', None, ptr, ptr, *([integer] * 8))
destroy = function('gdImageDestroy', None, ptr)
if version() != b'2.3.3':
    raise SystemExit('The resampling reference requires libgd 2.3.3')
cases=[]
for width, height, outw, outh in [(1,1,1,1),(2,1,1,1),(3,1,2,1),(17,9,7,3),(31,27,13,11)]:
    for alpha in [0,1,2,63,64,100,127,128,129,200,254,255,"mixed"]:
        pixels = [[(x*17+y*23)%256,(x*31+y*11)%256,(x*53+y*7)%256,(x*19+y*37)%256 if alpha == "mixed" else alpha] for y in range(height) for x in range(width)]
        source, output = create(width,height),create(outw,outh)
        assert source and output
        blend(source,0)
        background=[[255,255,238],[240,224,214],[238,242,255],[214,218,240]][len(cases)%4]
        for i, (r,g,b,a) in enumerate(pixels): set_pixel(source,i%width,i//width,((127-(a>>1))<<24)|(r<<16)|(g<<8)|b)
        for y in range(outh):
            for x in range(outw):set_pixel(output,x,y,(background[0]<<16)|(background[1]<<8)|background[2])
        resize(output,source,0,0,0,0,outw,outh,width,height)
        expected=[]
        for y in range(outh):
            for x in range(outw):
                p=get_pixel(output,x,y)
                expected += [(p>>16)&255,(p>>8)&255,p&255]
        cases.append(dict(size=[width,height],output=[outw,outh],background=background,alpha=alpha,expected=expected))
        destroy(source);destroy(output)
result = {
    'version': version().decode(),
    'reference': 'https://github.com/libgd/libgd/tree/gd-2.3.3',
    'scope': 'Synthetic pixel resampling; original deployed GD version is unknown',
    'cases': cases,
}
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--check', action='store_true')
args = parser.parse_args()
path = pathlib.Path(__file__).resolve().parents[1] / 'fixtures/gd-still-resampling.json'
if args.check:
    if json.loads(path.read_text()) != result:
        raise SystemExit('GD pixel reference differs')
    print(f'GD {result["version"]}: {len(cases)} resampling cases match')
else:
    print(json.dumps(result, separators=(',', ':')))
