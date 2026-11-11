// Copy an explicitly selected native DCIM video; never delete or transcode it.
import {createRequire} from 'node:module';
import {createWriteStream} from 'node:fs';
import {readFile, mkdir, writeFile, stat} from 'node:fs/promises';
import {pipeline} from 'node:stream/promises';
import {resolve, basename, relative, sep} from 'node:path';
import {fileURLToPath} from 'node:url';

const repo = resolve(fileURLToPath(new URL('../../../', import.meta.url)));
const root = resolve(repo, 'reports/app-review-mac');
const [configPath, source, outputDirectory] = process.argv.slice(2);
if (!configPath || !source || !outputDirectory) {
  throw Error('Usage: node export_original.mjs PRIVATE_CONFIG /DCIM/NNNAPPLE/IMG_NNNN.MP4 REPORTS_OUTPUT_DIRECTORY');
}
if (!/^\/DCIM\/\d+APPLE\/[^/]+\.(MP4|MOV)$/i.test(source)) throw Error('Select one DCIM video.');
const output = resolve(outputDirectory);
const inside = relative(resolve(repo, 'reports'), output);
if (!inside || inside.startsWith('..' + sep) || inside === '..') throw Error('Use a dedicated ignored reports output directory.');
const config = JSON.parse(await readFile(configPath, 'utf8'));
const require = createRequire(resolve(root, 'appium/node_modules/appium-xcuitest-driver/package.json'));
const {services} = require('appium-ios-device');
const afc = await services.startAfcService(config.udid);
try {
  const before = await afc.getFileInfo(source);
  await mkdir(output, {recursive:true, mode:0o700});
  const destination = resolve(output, basename(source));
  await pipeline(await afc.createReadStream(source, {autoDestroy:false}),
    createWriteStream(destination, {flags:'wx', mode:0o600}));
  const after = await afc.getFileInfo(source);
  const copied = await stat(destination);
  if (Number(before.size) !== copied.size || Number(after.size) !== copied.size) {
    throw Error('The source changed or export size differs; retain and inspect both files.');
  }
  const metadata = {source, destination, sourceInfo:after, bytes:copied.size, copiedAt:new Date().toISOString()};
  await writeFile(resolve(output, 'device-export.json'), JSON.stringify(metadata,null,2)+'\n', {flag:'wx',mode:0o600});
  console.log(JSON.stringify({destination,bytes:copied.size}));
} finally {afc.close();}
