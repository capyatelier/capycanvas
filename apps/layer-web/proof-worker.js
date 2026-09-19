import init, {proof_worker_build,tone_worker_build} from "./pkg/layer_web.js";
// Exactly one build, followed by host termination to release the Wasm arena.
self.onmessage=async({data})=>{
  try{await init();const result=data?.type==="tone"?tone_worker_build(data):proof_worker_build(data);self.postMessage({result},[result.bytes.buffer]);}
  catch(error){self.postMessage({error:String(error)});}
};
