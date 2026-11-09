'use strict';
// Independent verifier: built-in Node APIs only; never executes packaged binaries.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const assert = require('node:assert/strict');
const sha = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
function verify(directory, version, keyFile) {
  assert.match(version, /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/);
  const names = [`CodexFloatboard-${version}-windows-x64.zip`, `Floatboard-update-${version}-windows-x64.zip`, 'update-manifest.json', 'update-manifest.json.sig'];
  const read = name => {
    const file = path.join(directory, name);
    assert(fs.lstatSync(file).isFile() && !fs.lstatSync(file).isSymbolicLink(), `Not a regular asset: ${name}`);
    return fs.readFileSync(file);
  };
  const sums = read('SHA256SUMS.txt').toString('utf8').replace(/^\uFEFF/, '').trim().split(/\r?\n/);
  const seen = new Set();
  for (const line of sums) {
    const match = /^([a-fA-F0-9]{64})  ([A-Za-z0-9._-]+)$/.exec(line);
    assert(match && names.includes(match[2]) && !seen.has(match[2]), 'Unexpected/duplicate checksum entry');
    seen.add(match[2]); assert.equal(sha(read(match[2])), match[1].toLowerCase(), `Checksum mismatch: ${match[2]}`);
  }
  assert.equal(seen.size, names.length, 'Missing checksum entries');
  const rawKey = fs.readFileSync(keyFile, 'utf8').trim();
  assert.match(rawKey, /^[A-Za-z0-9+/]{43}=$/);
  const key = Buffer.from(rawKey, 'base64'); assert.equal(key.length, 32);
  const publicKey = crypto.createPublicKey({key: Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), key]), format: 'der', type: 'spki'});
  const manifestBytes = read('update-manifest.json');
  const sigText = read('update-manifest.json.sig').toString('utf8').trim();
  assert.match(sigText, /^[A-Za-z0-9+/]{86}==$/);
  assert(crypto.verify(null, manifestBytes, publicKey, Buffer.from(sigText, 'base64')), 'Ed25519 signature mismatch');
  const manifest = JSON.parse(manifestBytes.toString('utf8'));
  assert.equal(manifest.schema, 2); assert.equal(manifest.version, version);
  assert.equal(manifest.platform, 'windows-x86_64'); assert.equal(manifest.updater_protocol, 2);
  assert.match(manifest.minimum_version,/^\d+\.\d+\.\d+$/);assert.equal(manifest.agent_protocol,2);assert.equal(manifest.database_schema_min,2);assert.equal(manifest.database_schema_max,2);
  assert.equal(manifest.url, `https://github.com/Memento1990/codex-task/releases/download/v${version}/${names[1]}`);
  const update = read(names[1]); assert.equal(manifest.size, update.length); assert.equal(manifest.sha256, sha(update));
  return [...names, 'SHA256SUMS.txt'].map(name => {const bytes=read(name); return {name, size:bytes.length, sha256:sha(bytes)};});
}
function selfTest() {
  const os = require('node:os'); const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'floatboard-verify-fixture-'));
  try {
    const {publicKey, privateKey} = crypto.generateKeyPairSync('ed25519');
    const keyFile = path.join(dir, 'public.txt');
    fs.writeFileSync(keyFile, publicKey.export({format:'der',type:'spki'}).subarray(-32).toString('base64'));
    const version='1.2.3', portable=`CodexFloatboard-${version}-windows-x64.zip`, update=`Floatboard-update-${version}-windows-x64.zip`;
    fs.writeFileSync(path.join(dir,portable),'fixture portable'); fs.writeFileSync(path.join(dir,update),'fixture update');
    const bytes=fs.readFileSync(path.join(dir,update));
    const manifest=Buffer.from(JSON.stringify({schema:2,version,platform:'windows-x86_64',updater_protocol:2,minimum_version:version,database_schema_min:2,database_schema_max:2,agent_protocol:2,url:`https://github.com/Memento1990/codex-task/releases/download/v${version}/${update}`,size:bytes.length,sha256:sha(bytes)}));
    fs.writeFileSync(path.join(dir,'update-manifest.json'),manifest);
    fs.writeFileSync(path.join(dir,'update-manifest.json.sig'),crypto.sign(null,manifest,privateKey).toString('base64'));
    const names=[portable,update,'update-manifest.json','update-manifest.json.sig'];
    const sums=()=>fs.writeFileSync(path.join(dir,'SHA256SUMS.txt'),names.map(n=>`${sha(fs.readFileSync(path.join(dir,n)))}  ${n}`).join('\n'));
    sums(); assert.equal(verify(dir,version,keyFile).length,5);
    fs.appendFileSync(path.join(dir,update),'tamper'); assert.throws(()=>verify(dir,version,keyFile));
    sums(); assert.throws(()=>verify(dir,version,keyFile)); // matching sums cannot bypass signed manifest
    fs.writeFileSync(path.join(dir,update),bytes); sums();
    fs.writeFileSync(path.join(dir,'update-manifest.json.sig'),Buffer.alloc(64).toString('base64')); sums(); assert.throws(()=>verify(dir,version,keyFile));
    console.log('PASS: valid package, tamper, recomputed checksums, invalid signature');
  } finally { fs.rmSync(dir,{recursive:true,force:true}); }
}
if (require.main===module) {
  try {if(process.argv[2]==='--self-test') selfTest(); else {
    assert.equal(process.argv.length,5,'Usage: node verify-release.cjs DIRECTORY VERSION PINNED_KEY_FILE');
    console.log(JSON.stringify(verify(...process.argv.slice(2)),null,2));
  }} catch(error) {console.error(error.message);process.exitCode=1;}
}
module.exports={verify};
