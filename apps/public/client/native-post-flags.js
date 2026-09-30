// Finite classes from the pinned public flag sprites. No arbitrary CSS token.
const countries = new Set('ad ae af ag ai al am an ao aq ar as at au aw ax az ba bb bd be bf bg bh bi bj bl bm bn bo bq br bs bt bv bw by bz ca cc cd cf cg ch ci ck cl cm cn co cr cs cu cv cw cx cy cz de dj dk dm do dz ec ee eg eh xe er es et eu fi fj fk fm fo fr ga gb gd ge gf gg gh gi gl gm gn gp gq gr gs gt gu gw gy hk hm hn hr ht hu id ie il im in io iq ir is it je jm jo jp ke kg kh ki km kn kp kr kw ky kz la lb lc li lk lr ls lt lu lv ly ma mc md me mf mg mh mk ml mm mn mo mp mq mr ms mt mu mv mw mx my mz na nc ne nf ng ni nl no np nr nu nz om pa pe pf pg ph pk pl pm pn pr ps pt pw py qa re ro rs ru rw sa sb sc xs sd se sg sh si sj sk sl sm sn so sr ss st sv sx sy sz tc td tf tg th tj tk tl tm tn to tr tt tv tw tz ua ug um us uy uz va vc ve vg vi vn vu xw wf ws xk xx ye yt za zm zw'.split(' ').map(code => `flag-${code}`));
const boards = new Set('ac an bl cf cm ct dm eu fc gn gy jh kn mf nb nt nz pc pr re mz tm tr un wp'.split(' ').map(code => `bfl-${code}`));
export function isPostFlagToken(token) {
  return token === 'flag' || token === 'bfl' || countries.has(token) || boards.has(token);
}
export function isPostFlagClass(value) {
  const parts = value.split(' ');
  return parts.length === 2 && ((parts[0] === 'flag' && countries.has(parts[1]))
    || (parts[0] === 'bfl' && boards.has(parts[1])));
}
