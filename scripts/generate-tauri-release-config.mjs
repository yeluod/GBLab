import fs from 'node:fs';
import path from 'node:path';

const args = process.argv.slice(2);
const value = (name) => {
  const index = args.indexOf(name);
  if (index < 0 || !args[index + 1]) throw new Error(`Missing ${name}`);
  return args[index + 1];
};

const platform = value('--platform');
const sdkRoot = path.resolve(value('--sdk-root'));
const output = path.resolve(value('--output'));
const projectRoot = process.cwd();
const configPath = path.join(projectRoot, 'src-tauri', 'tauri.conf.json');
const config = JSON.parse(fs.readFileSync(configPath, 'utf8'));
const configPathValue = (filePath) => path.resolve(filePath);
const requiredResource = (filePath) => {
  const resolved = configPathValue(filePath);
  if (!fs.existsSync(resolved)) throw new Error(`Missing FFmpeg resource: ${resolved}`);
  return resolved;
};

if (platform === 'macos') {
  const frameworkRoot = path.join(sdkRoot, 'frameworks');
  const frameworks = fs
    .readdirSync(frameworkRoot)
    .filter((name) => name.endsWith('.dylib'))
    .sort()
    .map((name) => configPathValue(path.join(frameworkRoot, name)));
  if (frameworks.length === 0) throw new Error(`No macOS FFmpeg dylibs found in ${frameworkRoot}`);
  config.bundle.resources = [
    requiredResource(path.join(sdkRoot, 'manifest.json')),
    requiredResource(path.join(sdkRoot, 'FFMPEG-LICENSE.txt')),
  ];
  config.bundle.macOS = { ...(config.bundle.macOS ?? {}), frameworks };
} else if (platform === 'windows') {
  const binRoot = path.join(sdkRoot, 'bin');
  const dlls = fs.readdirSync(binRoot).filter((name) => name.toLowerCase().endsWith('.dll'));
  if (dlls.length === 0) throw new Error(`No Windows FFmpeg DLLs found in ${binRoot}`);
  config.bundle.resources = {
    [`${configPathValue(binRoot)}/*.dll`]: '',
    [requiredResource(path.join(sdkRoot, 'manifest.json'))]: '',
    [requiredResource(path.join(sdkRoot, 'FFMPEG-LICENSE.txt'))]: '',
  };
} else {
  throw new Error(`Unsupported platform: ${platform}`);
}

fs.mkdirSync(path.dirname(output), { recursive: true });
fs.writeFileSync(output, `${JSON.stringify(config, null, 2)}\n`);
console.log(`Generated ${output}`);
