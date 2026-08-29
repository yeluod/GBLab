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
const lockPath = path.join(projectRoot, 'toolchains', 'ffmpeg-sdk.lock.json');
const lock = JSON.parse(fs.readFileSync(lockPath, 'utf8'));
const manifestPath = requiredResource(path.join(sdkRoot, 'manifest.json'));
const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
const expectedArchitecture = process.arch === 'arm64' ? 'arm64' : 'x86_64';
if (manifest.ffmpegVersion !== lock.ffmpegVersion || manifest.sdkRevision !== lock.sdkRevision) {
  throw new Error('FFmpeg SDK does not match lockfile version or revision');
}
if (manifest.license !== lock.license || manifest.linkMode !== lock.linkMode) {
  throw new Error('FFmpeg SDK license or link mode does not match lockfile');
}
if (manifest.architecture !== expectedArchitecture) {
  throw new Error(
    'FFmpeg SDK architecture mismatch: expected ' +
      expectedArchitecture +
      ', got ' +
      manifest.architecture,
  );
}

if (platform === 'macos') {
  const frameworkRoot = path.join(sdkRoot, 'frameworks');
  const frameworks = fs
    .readdirSync(frameworkRoot)
    .filter((name) => name.endsWith('.dylib'))
    .sort()
    .map((name) => configPathValue(path.join(frameworkRoot, name)));
  if (frameworks.length === 0) throw new Error(`No macOS FFmpeg dylibs found in ${frameworkRoot}`);
  config.bundle.resources = [
    manifestPath,
    requiredResource(path.join(sdkRoot, 'FFMPEG-LICENSE.txt')),
  ];
  config.bundle.macOS = { ...(config.bundle.macOS ?? {}), frameworks };
} else if (platform === 'windows') {
  const binRoot = path.join(sdkRoot, 'bin');
  const dlls = fs.readdirSync(binRoot).filter((name) => name.toLowerCase().endsWith('.dll'));
  if (dlls.length === 0) throw new Error(`No Windows FFmpeg DLLs found in ${binRoot}`);
  config.bundle.resources = {
    [`${configPathValue(binRoot)}/*.dll`]: '',
    [manifestPath]: '',
    [requiredResource(path.join(sdkRoot, 'FFMPEG-LICENSE.txt'))]: '',
  };
} else {
  throw new Error(`Unsupported platform: ${platform}`);
}

fs.mkdirSync(path.dirname(output), { recursive: true });
fs.writeFileSync(output, `${JSON.stringify(config, null, 2)}\n`);
console.log(`Generated ${output}`);
