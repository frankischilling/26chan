// Owned synthetic qualification transcripts, not uploads or a general admission policy.
const stroke = (x=3,y=4,x2=19,y2=17) => [[1,x,y,49151],[2,x2,y2,32768],[3]];
const noP = (x=0,y=0,x2=23,y2=23) => [[7,x,y],[8,x2,y2],[3]];
const make = (id, events, options={}) => Object.freeze({id,width:24,height:24,toolId:1,...options,events:[[0],...events,[255]]});
export const CASES = Object.freeze([
  make('pencil-pressure',stroke()),
  ...[1,2,3,5,7,8].map(id=>make(`tool-${id}`,[
    [11,9],...stroke(),[6,28,115,207],[10,id],[11,7],[12,.625],[18,.375],...noP(),[11,1],...stroke(23,0,0,23)])),
  ...[0,1,2].map(tip=>make(`eraser-tip-${tip}`,[[11,16],...stroke(),[10,8],[15,tip],[11,8],[12,.375],...noP()])),
  make('tone-initial-and-settings',[...stroke(),[12,0],...noP(),[12,.5],[12,.5],[18,0],...stroke(),[10,1],[10,5],[16,1],...noP(),[16,0]],{toolId:5}),
  make('alpha-flow-preserve',[[11,12],...stroke(),[10,2],[12,0],[18,0],...noP(),
    [12,Math.fround(1/255)],[18,Math.fround(1/255)],...stroke(),[12,.625],[18,.75],[16,1],[6,199,37,93],...noP(),[16,0],[12,1],[18,1],[11,64],...stroke(0,23,23,0)]),
  make('layer-history-order',[...stroke(),[20],[6,200,32,16],...noP(),[27,.375],[4],[5],[20],[25,1],[20],[6,12,201,75],...stroke(),[25,0],[26,2],[27,.625],[27,.75],[4],[5],[24,2],[24,2],[254],[4],[5],[25,2],[4],[5]]),
  make('alpha-coalesce-redo',[...stroke(),[27,.5],...noP(),[4],[27,.75],[5],[4],[4],[5],[5]]),
  make('eight-layers',[[20],[20],[20],[20],[20],[20],[20],[25,4],...stroke(),[27,.5],[24,4],[25,0],...noP()]),
  make('history-eviction',[...stroke(),...Array.from({length:52},()=>[254]),[4],[5]]),
  make('asymmetric-blur',[[11,7],...stroke(0,0,12,4),[10,7],[11,3],...noP(12,0,0,4),...stroke(0,4,12,0)],{width:13,height:5}),
  make('tiny-pen',[[11,1],...stroke(0,0,0,0),...noP(0,0,0,0)],{width:1,height:1,toolId:2}),
]);
// Exact source defaults independently frozen in replay-cost/tools.tsv.
const TOOLS=[[1,1,1,.01,1,1],[2,8,1,.05,1,1],[3,32,1,.1,1,1],[4,1,1,100,1,0],[5,8,.5,.01,1,1],[6,1,1,100,1,0],[7,32,.5,.25,1,0],[8,8,1,.1,1,0]];
export function encodeOwnedCase(item){
  const out=new Uint8Array(256+item.events.length*16),d=new DataView(out.buffer);
  out.set(new TextEncoder().encode('IBRPLY01'));
  [1,64,1,0].forEach((n,i)=>d.setUint16(8+i*2,n));
  d.setUint32(16,out.length);d.setUint32(20,item.events.length);d.setUint16(24,item.width);d.setUint16(26,item.height);
  out.set([240,246,251,43,61,79,item.toolId],28);d.setUint32(36,1700000000);d.setUint32(40,1700000001);out.set([0,9,4,1],44);
  for(const [id,size,alpha,step,flow,preserve] of TOOLS){const p=64+(id-1)*24;out.set([id,size,preserve*4,0],p);d.setFloat32(p+4,alpha);d.setFloat32(p+8,step);d.setFloat32(p+12,flow);}
  item.events.forEach(([tag,...args],i)=>{const p=256+i*16;out[p]=tag;d.setUint32(p+4,i*7);
    if([1,2,7,8].includes(tag)){d.setInt16(p+8,args[0]);d.setInt16(p+10,args[1]);if(tag===1||tag===2)d.setUint16(p+12,args[2]);}
    else if(tag===6)out.set(args,p+8);else if([12,18,27].includes(tag))d.setFloat32(p+8,args[0]);else if(args.length)out[p+8]=args[0];});
  return out;
}
