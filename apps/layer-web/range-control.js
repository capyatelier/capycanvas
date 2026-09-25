import { createNumberField } from './numeric.js';

// Shared panel/toolbar presentation. Rust owns numeric resolution and edits;
// the browser owns contact capture, focus and native keyboard adjustment.
export function createRangeControl({ app, bounds, label, icon, onChange, prefix = 'tool', showSlider = true }) {
  const root = document.createElement('div'); root.className = 'range-control'; root.title = label;
  root.dataset.range = prefix;
  const track = document.createElement('div'); track.className = 'interval-track'; track.hidden = !showSlider;
  const trough = document.createElement('div'); trough.className = 'interval-trough';
  const fill = document.createElement('div'); fill.className = 'interval-fill';
  trough.append(fill); track.append(trough);
  let values = bounds.map(f => f.value), domain, contact, retired = false;
  const spec = bounds[0].numeric;
  const inputs = bounds.map((f, i) => {
    const input = createNumberField({ control: f.numeric, label: `${f.label} — ${label}`, icon, inline: true, valueOnly: true,
      resolve: request => app.number_input(request), onChange: value => { if (!retired) onChange(i, value); } });
    input.dataset[`${prefix}Setting`] = f.id;
    return input;
  });
  const handles = bounds.map((f, i) => {
    const input = document.createElement('input'); input.type = 'range'; input.className = 'interval-input';
    input.step = f.numeric.step; input.dataset.rangeHandle = f.id;
    input.setAttribute('aria-label', `${f.label} — ${label}`); input.title = input.getAttribute('aria-label');
    const thumb = document.createElement('span'); thumb.className = `interval-thumb ${i ? 'upper' : 'lower'}`;
    track.append(input, thumb);
    input.oninput = () => change(i, Number(input.value));
    return { input, thumb };
  });
  root.append(inputs[0], track, inputs[1]);
  function paint() {
    domain = contact?.domain || [Math.min(spec.soft_min, values[0]), Math.max(spec.soft_max, values[1])];
    const positions = values.map(v => Math.max(0, Math.min(1, (v-domain[0])/(domain[1]-domain[0]))));
    for (let i = 0; i < 2; i++) {
      inputs[i].update(values[i]);
      const {input, thumb} = handles[i];
      input.min = i ? values[0] : spec.min; input.max = i ? spec.max : values[1]; input.value = values[i];
      input.setAttribute('aria-valuetext', app.number_input({control: spec, value: values[i], operation: {type:'format'}}).text);
      thumb.style.left = `calc(8px + (100% - 16px) * ${positions[i]})`;
    }
    fill.style.left = `${positions[0]*100}%`; fill.style.width = `${(positions[1]-positions[0])*100}%`;
  }
  function change(index, value) {
    if (retired) return;
    const resolved = app.number_input({control: spec, value: values[index], operation:{type:'value',value}}).value;
    const next = index ? Math.max(values[0], resolved) : Math.min(values[1], resolved);
    if (next === values[index]) return;
    values[index] = next; paint(); onChange(index, next);
  }
  function pick(e) {
    if (contact?.id !== e.pointerId) return;
    const b = track.getBoundingClientRect();
    const control = {...spec, soft_min: contact.domain[0], soft_max: contact.domain[1]};
    change(contact.index, app.number_input({control, value: contact.before, operation:{type:'position',position:(e.clientX-b.x-contact.offset-8)/Math.max(1,b.width-16)}}).value);
  }
  function end(cancel) {
    const c = contact; if (!c) return;
    contact = null;
    if (cancel) change(c.index, c.before);
    if (track.hasPointerCapture(c.id)) track.releasePointerCapture(c.id);
    paint();
  }
  track.addEventListener('pointerdown', e => {
    if (e.button || contact || retired) return;
    e.preventDefault(); e.stopPropagation();
    inputs.forEach(n => n.cancelEditing());
    const b = track.getBoundingClientRect(), x = e.clientX-b.x;
    const positions = values.map(v => 8+(v-domain[0])/(domain[1]-domain[0])*(b.width-16));
    const index = Math.abs(positions[1]-positions[0]) < 1 ? Number(x>=positions[0]) : Number(Math.abs(x-positions[1])<Math.abs(x-positions[0]));
    contact = {id:e.pointerId,index,before:values[index],domain:[...domain],offset:Math.abs(x-positions[index])<=12 ? x-positions[index] : 0};
    track.setPointerCapture(e.pointerId); handles[index].input.focus({preventScroll:true}); pick(e);
  });
  track.addEventListener('pointermove', e => { if (contact?.id === e.pointerId) { e.preventDefault(); pick(e); } });
  track.addEventListener('pointerup', e => { if (contact?.id === e.pointerId) end(false); });
  for (const type of ['pointercancel','lostpointercapture']) track.addEventListener(type, e => { if (contact?.id === e.pointerId) end(true); });
  root.addEventListener('keydown', e => { if (e.key === 'Escape' && contact) { e.preventDefault(); e.stopPropagation(); end(true); } });
  const blur = () => end(true); window.addEventListener('blur', blur);
  root.update = next => { values = [...next]; paint(); };
  root.dispose = () => { retired = true; contact = null; inputs.forEach(n => n.cancelEditing()); window.removeEventListener('blur', blur); };
  paint(); return root;
}
