import { CASES } from './cases.mjs';
const stroke = [[1,3,4,49151],[2,19,17,32768],[3]];
const make = (id, events) => ({id,width:24,height:24,toolId:1,events:[[0],...events,[255]]});
export const FULL_CASES = [
  ...CASES,
  make('bucket-pipette', [[10,4],...stroke,[10,6],...stroke,[10,1],...stroke]),
  make('brush-dynamics', [[10,2],[13,1],[14,1],[17,1],...stroke,[13,0],[14,0],[17,0],...stroke]),
  make('delete-and-undo', [...stroke,[20],...stroke,[21],[4],[5],[4]]),
  make('merge-and-undo', [...stroke,[20],[6,255,0,0],...stroke,[23],[4],[5]]),
  make('move-and-undo', [[20],[20],...stroke,[22,0],[4],[5]]),
  make('undo-add', [[20],[4],[5],...stroke]),
];
