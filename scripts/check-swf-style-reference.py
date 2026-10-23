#!/usr/bin/env python3
"""Verify the supplied upload-table CSS slices, without fetching page resources."""
import argparse,hashlib,json,re
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
PINS={'yotsuba': ['05d61b9a886aca60bd418b63a5b86d2ef3050d1d90c640da64f2206dda740db9', 'fc9bcf2c7b5e51c72d71acd0e054875b0b766a52bf1f26353ac4af3b70119410'], 'yotsuba-b': ['d6abc7c9fb8c2d9c168554b39a54b4569f68243aa4bcb972b8452264cbdd1a65', '95103908737e7006a91a0b5f230e6def39c8e28fdacf2af25d6103384543dfaf'], 'futaba': ['8beb159cfcf83d042a68b1037997511c7b0bb959289eb1b5757036cdd7aa0945', 'b2f8aedd1b458dfbc7e1d0b9a2daf614aac48de41617759392d368e8692e478c'], 'burichan': ['71a5a5b8362aa78269c19e89cacc249cda48f2af2393fda7d3638fa518307b18', '44f3f0342597590b37b81a66c9f7d5dda509c4c26fe4184b5bf6889fd0db46aa'], 'photon': ['a4741759760d959b4a2e29d9622ab63a91d1cdc49f10679335b71f5b16e9575d', '28671bac8b2901ceeac2b63774a3b0ef31b7f44c9a5e1a647f669aed3dada0fa'], 'tomorrow': ['40b4c474a93e87737811c77a50cce9af4fadc9b025a3863e126ca418a8f33a4a', 'f346b7359d6bf77d24bb0738a35b6cdbdf9e998610e75fac15bf70e27b76c9a3']}
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('source',type=Path,nargs='?')
args=parser.parse_args()
fixture=json.loads((ROOT/'fixtures/swf-style-reference.json').read_text())
assert set(fixture['themes'])==set(PINS)
selectors={'a','.postblock','table.flashListing','table.flashListing td.postblock','table.flashListing td','table.flashListing td:not(.subject)','table.flashListing .name','table.flashListing .postertrip','table.flashListing .subject','table.flashListing tr:nth-of-type(odd)','.highlightPost:not(.op)','.name-col, .file-col, table.flashListing .subject'}
for theme,row in fixture['themes'].items():
    assert row['file_sha256']==PINS[theme][0]
    assert row['css_sha256']==PINS[theme][1]
    assert hashlib.sha256(row['css'].encode()).hexdigest()==PINS[theme][1]
    assert 'url(' not in row['css'] and '</' not in row['css'] and '@import' not in row['css']
    if args.source:
        raw=(args.source/row['path']).read_bytes()
        assert hashlib.sha256(raw).hexdigest()==PINS[theme][0]
        text=re.sub(r'/\*.*?\*/','',raw.decode().replace('\r\n','\n'),flags=re.S)
        blocks=[];links=[]
        for match in re.finditer(r'([^{}]+)\{([^{}]*)\}',text):
            selector=' '.join(match[1].split()).strip()
            if selector=='body':
                props=[prop.strip()+';' for prop in match[2].split(';') if ':' in prop and prop.split(':',1)[0].strip() in ['font','font-family','font-size','line-height','color']]
                blocks.append('body { '+' '.join(props)+' }')
            elif selector in selectors: blocks.append(selector+' {'+match[2]+'}')
            elif selector in ['a, a:visited','a:hover']: links.append(selector+' {'+match[2]+'}')
        assert '\n'.join(links)+'\n'+'\n'.join(blocks)+'\n'==row['css']
print('Verified upload-table style rules for all six source themes.')
