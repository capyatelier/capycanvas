// One recording per window, shared by retained diagnostic panels and drawers.
const controllers = new WeakMap();
export function strokeRecordingControl(app, button, message = console.error) {
  let controller = controllers.get(app);
  if (!controller) {
    let busy = false, wasActive = false;
    const buttons = new Set();
    const update = () => {
      const state = app.stroke_recording_status();
      for (const b of buttons) {
        b.textContent = state.label;
        b.disabled = busy;
        b.title = `Record tablet input for up to 10 minutes · ${state.elapsed_seconds}s · ${state.raw_events} inputs`;
      }
      if (wasActive && state.ready && !busy) {
        // File pickers require a fresh user gesture after an automatic stop.
        const dialog = document.createElement('dialog');
        const text = document.createElement('p');
        text.textContent = 'Stroke recording finished. Save the recorded input.';
        const save = document.createElement('button'); save.textContent = 'Save recording';
        const later = document.createElement('button'); later.textContent = 'Later';
        save.onclick = () => { dialog.close(); void deliver(); };
        later.onclick = () => dialog.close();
        dialog.onclose = () => dialog.remove();
        dialog.append(text, later, save); document.body.append(dialog); dialog.showModal();
      }
      wasActive = state.recording;
    };
    async function deliver() {
      if (busy) return;
      busy = true; update();
      let writable;
      try {
        const name = 'stroke-recording.capystrokes';
        // Invoke the system picker directly in the click's activation turn.
        const handle = window.showSaveFilePicker ? await window.showSaveFilePicker({suggestedName:name,
          types:[{description:'Stroke recording',accept:{'application/octet-stream':['.capystrokes']}}]}) : null;
        const gzip = new Blob([app.stroke_recording_data()]).stream().pipeThrough(new CompressionStream('gzip'));
        const bytes = new Uint8Array(await new Blob(['CAPYPEN2', await new Response(gzip).blob()]).arrayBuffer());
        if (handle) {
          writable = await handle.createWritable(); await writable.write(bytes); await writable.close(); writable = null;
          app.stroke_recording_saved();
        } else {
          // Android Chrome has no save-file picker. Hand the file to its system
          // share sheet when supported, otherwise use the browser's download UI.
          const file = new File([bytes], name, {type:'application/octet-stream'});
          if (navigator.canShare?.({files:[file]})) {
            await navigator.share({files:[file], title:'Stroke recording'});
            app.stroke_recording_saved();
          } else {
            const url = URL.createObjectURL(file), dialog = document.createElement('dialog');
            const link = document.createElement('a'); link.href = url; link.download = name; link.textContent = 'Download stroke recording';
            const done = document.createElement('button'); done.textContent = 'File saved'; done.disabled = true;
            const later = document.createElement('button'); later.textContent = 'Keep for later';
            link.onclick = () => { done.disabled = false; };
            done.onclick = () => { app.stroke_recording_saved(); dialog.close(); update(); };
            later.onclick = () => dialog.close();
            dialog.onclose = () => { URL.revokeObjectURL(url); dialog.remove(); };
            dialog.append(link, later, done); document.body.append(dialog); dialog.showModal();
          }
        }
      } catch (error) {
        if (writable) await writable.abort().catch(()=>{});
        if (error.name !== 'AbortError') message(error);
      } finally { busy = false; update(); }
    }
    controller = {buttons, update, click() {
      if (busy) return;
      try {
        const state = app.stroke_recording_status();
        if (state.recording) { wasActive = false; app.stop_stroke_recording(); void deliver(); }
        else if (state.ready) void deliver();
        else { app.start_stroke_recording(); update(); }
      } catch (error) { message(error); }
    }};
    // This timer survives panel hiding and stops an idle capture at the limit.
    setInterval(update, 200); controllers.set(app, controller);
  }
  controller.buttons.add(button); button.dataset.control = 'stroke-recording';
  button.onclick = controller.click; controller.update();
  return () => controller.buttons.delete(button);
}
