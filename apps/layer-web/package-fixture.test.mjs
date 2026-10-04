import assert from 'node:assert/strict';

export const authoredIdentity=manifest=>{
  const objects=structuredClone(manifest.objects);
  for(const object of objects)if(object.type==='capy.output/1') {
    assert.ok(Number.isFinite(object.data.context.elapsed)&&object.data.context.elapsed>=0,'Output capture elapsed time is finite and nonnegative');
    object.data.context.elapsed=0;
  }
  return objects;
};
const evidenceByManifest = new WeakMap();

export async function packageManifest(bytes) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const text = new TextDecoder(), maxMetadata = 64 * 1024 * 1024;
  const crcTable = Uint32Array.from({length:256}, (_, index) => {
    let crc = index;
    for (let bit = 0; bit < 8; bit++) crc = (crc >>> 1) ^ ((crc & 1) ? 0xedb88320 : 0);
    return crc >>> 0;
  });
  let at = 0;
  while (at + 30 <= bytes.length && view.getUint32(at, true) === 0x04034b50) {
    const method = view.getUint16(at + 8, true);
    if (view.getUint16(at + 6, true) !== 0) throw Error('Unsupported package member flags');
    const nameLength = view.getUint16(at + 26, true), extraLength = view.getUint16(at + 28, true);
    const extra = at + 30 + nameLength, start = extra + extraLength;
    if (start > bytes.length) throw Error('Incomplete package member header');
    const name = text.decode(bytes.subarray(at + 30, extra));
    let length = view.getUint32(at + 18, true), decodedLength = view.getUint32(at + 22, true);
    if (length === 0xffffffff || decodedLength === 0xffffffff) {
      const fields = Number(decodedLength === 0xffffffff) + Number(length === 0xffffffff);
      if (extraLength !== 4 + fields * 8 || view.getUint16(extra, true) !== 1
          || view.getUint16(extra + 2, true) !== fields * 8) throw Error('Missing ZIP64 member length');
      let offset = extra + 4;
      if (decodedLength === 0xffffffff) { decodedLength = Number(view.getBigUint64(offset, true)); offset += 8; }
      if (length === 0xffffffff) length = Number(view.getBigUint64(offset, true));
    }
    const end = start + length;
    if (!Number.isSafeInteger(length) || !Number.isSafeInteger(decodedLength) || !Number.isSafeInteger(end)
        || end > bytes.length) throw Error('Incomplete package member');
    if (method === 0 ? length !== decodedLength : method !== 8 || name !== 'manifest.json' || length >= decodedLength)
      throw Error('Unsupported package member compression');
    if (at === 0 && (name !== 'mimetype' || method !== 0
        || text.decode(bytes.subarray(start, end)) !== 'application/vnd.capycanvas')) throw Error('Missing Capy package mimetype');
    if (name === 'manifest.json') {
      if (decodedLength > maxMetadata) throw Error('Authored manifest exceeds metadata limit');
      let decoded = bytes.subarray(start, end);
      if (method === 8) {
        decoded = new Uint8Array(decodedLength);
        const reader = new Blob([bytes.subarray(start, end)]).stream().pipeThrough(new DecompressionStream('deflate-raw')).getReader();
        let offset = 0;
        try {
          for (;;) {
            const {value, done} = await reader.read();
            if (done) break;
            if (value.length > decodedLength - offset) throw Error('Authored manifest exceeds declared length');
            decoded.set(value, offset); offset += value.length;
          }
          if (offset !== decodedLength) throw Error('Incomplete authored manifest');
        } catch (error) {
          await reader.cancel().catch(() => {});
          throw error;
        } finally { reader.releaseLock(); }
      }
      let crc = 0xffffffff;
      for (const byte of decoded) crc = (crc >>> 8) ^ crcTable[(crc ^ byte) & 255];
      if (((crc ^ 0xffffffff) >>> 0) !== view.getUint32(at + 14, true)) throw Error('Authored manifest CRC mismatch');
      const manifest = JSON.parse(text.decode(decoded));
      if (manifest.format !== 'capy.canvas' || manifest.version !== 1) throw Error('Unexpected authored manifest');
      return manifest;
    }
    at = end;
  }
  throw Error('Missing authored package manifest');
}

export async function packageEvidence(bytes, manifest) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength), text = new TextDecoder(), members = new Map();
  let at = 0;
  while (at + 30 <= bytes.length && view.getUint32(at, true) === 0x04034b50) {
    const names = view.getUint16(at + 26, true), extras = view.getUint16(at + 28, true);
    const name = text.decode(bytes.subarray(at + 30, at + 30 + names));
    let length = view.getUint32(at + 18, true);
    if (length === 0xffffffff) length = Number(view.getBigUint64(at + 30 + names + 12, true));
    const start = at + 30 + names + extras, end = start + length;
    if (!Number.isSafeInteger(end) || end > bytes.length) throw Error('Incomplete package resource pack');
    members.set(name, [start, length]); at = end;
  }
  const result = {};
  for (const resource of manifest.resources) {
    const member = members.get(resource.location.pack), offset = Number(resource.location.offset), length = Number(resource.bytes);
    if (!member || !Number.isSafeInteger(offset + length) || offset < 0 || offset + length > member[1]) throw Error('Invalid fixture resource range');
    const hashes = [];
    for (let begin = 0; begin < length; begin += 4 * 1024 * 1024) {
      const block = bytes.subarray(member[0] + offset + begin, member[0] + offset + Math.min(length, begin + 4 * 1024 * 1024));
      const digest = new Uint8Array(await crypto.subtle.digest('SHA-256', block));
      hashes.push(Array.from(digest, byte => byte.toString(16).padStart(2, '0')).join(''));
    }
    result[resource.id] = hashes;
  }
  return result;
}
export async function readPackage(evaluate, expression) {
  const [manifest, evidence] = await evaluate(`(async()=>{const bytes=${expression};const manifest=await (${packageManifest.toString()})(bytes);return [manifest,await (${packageEvidence.toString()})(bytes,manifest)]})()`);
  evidenceByManifest.set(manifest, evidence);
  return manifest;
}

export const packageObject = (manifest, reference) => {
  const id = typeof reference === 'string' ? reference : reference.ref;
  const object = manifest.objects.find(object => object.id === id);
  if (!object) throw Error('Missing authored object ' + id);
  return object;
};
export const packageObjects = (manifest, type) => manifest.objects.filter(object => object.type === type);
export const packageComposition = manifest => packageObject(manifest, manifest.root);
export const packageOutput = manifest => packageObject(manifest, manifest.default_output);
export function packageOccurrences(manifest) {
  const result = [];
  const visit = stack => {
    for (const reference of packageObject(manifest, stack).data.entries ?? []) {
      const occurrence = packageObject(manifest, reference);
      result.push(occurrence);
      if (occurrence.data.content.stack) visit(occurrence.data.content.stack);
    }
  };
  visit(packageComposition(manifest).data.result.object);
  return result;
}
export const packageResources = (manifest, type) => manifest.resources.filter(resource => !type || resource.type === type);
export function resourceIdentity(manifest, reference) {
  const resource = manifest.resources.find(resource => resource.id === reference.ref);
  if (!resource) throw Error('Missing immutable resource ' + reference.ref);
  const {location, ...identity} = resource;
  const hashes = evidenceByManifest.get(manifest)?.[resource.id];
  if (!hashes) throw Error('Read immutable resource identity with bounded payload evidence');
  return {...identity, sha256: hashes};
}
export const packageResourceIdentity = manifest => manifest.resources.map(resource => resourceIdentity(manifest, {ref: resource.id}));
function sourceResource(manifest, reference, content, samples = false) {
  const identity = resourceIdentity(manifest, reference), data = {...identity.data};
  if (samples) delete data.profile;
  else if (data.profile) data.profile = sourceResource(manifest, data.profile, content);
  const result = {...identity, data};
  if (content) delete result.id;
  return result;
}
function sourceOriginals(manifest, content) {
  return packageObjects(manifest, 'capy.paint-source/1').filter(source => source.data.original).map(source => {
    const original = source.data.original, interpretation = {...original.interpretation};
    if (interpretation.profile.resource) interpretation.profile = {
      ...interpretation.profile, resource: sourceResource(manifest, interpretation.profile.resource, content),
    };
    return {...original, interpretation, tiles: original.tiles.map(tile => ({
      ...tile, resource: sourceResource(manifest, tile.resource, content),
    }))};
  });
}
export const sourceIdentity = manifest => sourceOriginals(manifest, false);
export const sourceContent = manifest => sourceOriginals(manifest, true);
export const sourceSamples = manifest => packageObjects(manifest, 'capy.paint-source/1').filter(source => source.data.original).map(source =>
  source.data.original.tiles.map(tile => ({...tile, resource: sourceResource(manifest, tile.resource, true, true)})));
export const rasterIdentity = manifest => manifest.objects.filter(source => ['capy.paint-source/1', 'capy.coverage-source/1'].includes(source.type)).map(source => ({
  id: source.id, type: source.type, domain: source.data.domain,
  tiles: (source.data.tiles ?? []).map(tile => ({...tile, resource: resourceIdentity(manifest, tile.resource)})),
  ...(source.data.material ? {material: source.data.material} : {}),
}));
