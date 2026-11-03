export function middleEllipsis(text,width,measure) {
  if(width<=0)return '';
  if(measure(text)<=width)return text;
  if(measure('…')>width)return '';
  const letters=[...new Intl.Segmenter(undefined,{granularity:'grapheme'}).segment(text)].map(part=>part.segment);
  const at=count=>letters.slice(0,Math.ceil(count/2)).join('')+'…'+(count>1?letters.slice(-Math.floor(count/2)).join(''):'');
  let low=0,high=letters.length;
  while(low<high){const middle=Math.ceil((low+high)/2);if(measure(at(middle))<=width)low=middle;else high=middle-1;}
  return at(low);
}
