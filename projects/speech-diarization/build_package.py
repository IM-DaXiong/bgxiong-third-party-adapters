"""Build immutable distributable resource bytes from an explicitly prepared runtime."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import zipfile

ROOT=Path(__file__).resolve().parent
def digest(path):
    with path.open('rb') as stream:return hashlib.file_digest(stream,'sha256').hexdigest()
def package_adapter():
    resource_path=ROOT/'companion/resources.lock.json'
    resource=json.loads(resource_path.read_text(encoding='utf-8'))
    resource['files']=[item for item in resource['files'] if item['path'].startswith('models/')]
    for file in sorted((ROOT/'companion').rglob('*')):
        if file.is_file() and file.suffix in ('.py','.ps1') and 'models' not in file.parts:
            resource['files'].append({'path':file.relative_to(ROOT/'companion').as_posix(),'sha256':digest(file)})
    resource_path.write_text(json.dumps(resource,indent=2),encoding='utf-8')
    pack=ROOT/'adapter-pack';pack.mkdir(exist_ok=True)
    shutil.copy2(ROOT/'adapter/target/wasm32-wasip2/release/bgx_speech_diarization.wasm',pack/'adapter.wasm')
    manifest={'schemaVersion':2,'pluginId':'local.speech.diarization','version':'1.0.0','displayName':'3D-Speaker CAM++ / FSMN',
        'entry':'adapter.wasm','language':'rust','protocolRange':{'min':2,'max':2},'artifactSha256':digest(pack/'adapter.wasm'),
        'publisherId':None,'publisherKeyId':None,'requiredFeatures':[],'optionalFeatures':[],
        'requestedCapabilities':{'endpointPolicy':['userConfigurable','allowPrivateNetwork'],'networkHosts':[],'allowPrivateNetwork':True,'auth':['none'],
                                 'mediaSlots':['audio'],'stateBytes':0,'embeddedHeaders':[]},
        'limits':{'memoryBytes':67108864,'fuel':100000000,'wallMs':180000,'outputBytes':16777216}}
    (pack/'manifest.json').write_text(json.dumps(manifest,indent=2),encoding='utf-8')
    (pack/'manifest.sha256').write_text(manifest['artifactSha256']+' adapter.wasm\n'+digest(pack/'manifest.json')+' manifest.json\n',encoding='ascii')
    return pack,manifest
def main():
    parser=argparse.ArgumentParser();parser.add_argument('--runtime',type=Path);parser.add_argument('--out',type=Path);args=parser.parse_args()
    pack,manifest=package_adapter()
    if not args.runtime:return
    args.out.mkdir(parents=True,exist_ok=True)
    name='speech-diarization-1.0.0-windows-x64.zip';archive=args.out/name
    if archive.exists():raise RuntimeError('Refusing to overwrite frozen artifact')
    with zipfile.ZipFile(archive,'w',compression=zipfile.ZIP_DEFLATED,compresslevel=6) as output:
        output.write(ROOT.parents[1]/'LICENSE','LICENSE')
        for origin,prefix in ((pack,'adapter-pack'),(ROOT/'companion','companion'),(args.runtime/'python','python')):
            for file in sorted(origin.rglob('*')):
                if file.is_file() and '__pycache__' not in file.parts and file.suffix not in ('.pyc','.pdb'):
                    output.write(file,prefix+'/'+file.relative_to(origin).as_posix())
    descriptor={'pluginId':manifest['pluginId'],'version':manifest['version'],
        'url':'https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/speech-diarization-1.0.0/'+name,
        'sha256':digest(archive),'bytes':archive.stat().st_size,'adapterSha256':manifest['artifactSha256']}
    (args.out/'resource.json').write_text(json.dumps(descriptor,indent=2),encoding='utf-8')
    (args.out/(name+'.sha256')).write_text(descriptor['sha256']+'  '+name+'\n',encoding='ascii')
    print(json.dumps(descriptor))
if __name__=='__main__':main()
