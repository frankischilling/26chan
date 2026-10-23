//! Exact source encoding for unchanged imported descriptions. Operator edits
//! remain their own literal text; this lookup never supplies template markup.
pub(super) fn description<'a>(board: &str, current: &'a str) -> &'a str {
    match (board, current) {
        ("3", "\"/3/ - 3DCG\" is 4chan's board for 3D modeling and imagery.") => {
            "&quot;/3/ - 3DCG&quot; is 4chan's board for 3D modeling and imagery."
        }
        (
            "a",
            "\"/a/ - Anime & Manga\" is 4chan's imageboard dedicated to the discussion of Japanese animation and manga.",
        ) => {
            "&quot;/a/ - Anime &amp; Manga&quot; is 4chan's imageboard dedicated to the discussion of Japanese animation and manga."
        }
        (
            "aco",
            "\"/aco/ - Adult Cartoons\" is 4chan's imageboard for posting western-styled adult cartoon artwork.",
        ) => {
            "&quot;/aco/ - Adult Cartoons&quot; is 4chan's imageboard for posting western-styled adult cartoon artwork."
        }
        ("adv", "\"/adv/ - Advice\" is 4chan's board for giving and receiving advice.") => {
            "&quot;/adv/ - Advice&quot; is 4chan's board for giving and receiving advice. "
        }
        (
            "an",
            "\"/an/ - Animals & Nature\" is 4chan's imageboard for posting pictures of animals, pets, and nature.",
        ) => {
            "&quot;/an/ - Animals &amp; Nature&quot; is 4chan's imageboard for posting pictures of animals, pets, and nature."
        }
        (
            "asp",
            "\"/asp/ - Alternative Sports & Wrestling\" is 4chan's imageboard for the discussion of alternative and extreme sports such as wrestling and paintball.",
        ) => {
            "&quot;/asp/ - Alternative Sports &amp; Wrestling&quot; is 4chan's imageboard for the discussion of alternative and extreme sports such as wrestling and paintball."
        }
        (
            "b",
            "\"/b/ - Random\" is the birthplace of Anonymous, and where people go to discuss random topics and create memes on 4chan.",
        ) => {
            "&quot;/b/ - Random&quot; is the birthplace of Anonymous, and where people go to discuss random topics and create memes on 4chan."
        }
        (
            "bant",
            "\"/bant/ - International/Random\" is 4chan's international hanging out board, where you can have fun with Anonymous all over the world.",
        ) => {
            "&quot;/bant/ - International/Random&quot; is 4chan's international hanging out board, where you can have fun with Anonymous all over the world."
        }
        (
            "biz",
            "\"/biz/ - Business & Finance\" is 4chan's imageboard for the discussion of business and finance, and cryptocurrencies such as Bitcoin and Dogecoin.",
        ) => {
            "&quot;/biz/ - Business &amp; Finance&quot; is 4chan's imageboard for the discussion of business and finance, and cryptocurrencies such as Bitcoin and Dogecoin."
        }
        ("c", "\"/c/ - Anime/Cute\" is 4chan's imageboard for cute and moe anime images.") => {
            "&quot;/c/ - Anime/Cute&quot; is 4chan's imageboard for cute and moe anime images."
        }
        (
            "cgl",
            "\"/cgl/ - Cosplay & EGL\" is 4chan's imageboard for the discussion of cosplay, elegant gothic lolita (EGL), and anime conventions.",
        ) => {
            "&quot;/cgl/ - Cosplay &amp; EGL&quot; is 4chan's imageboard for the discussion of cosplay, elegant gothic lolita (EGL), and anime conventions."
        }
        (
            "ck",
            "\"/ck/ - Food & Cooking\" is 4chan's imageboard for food pictures and cooking recipes.",
        ) => {
            "&quot;/ck/ - Food &amp; Cooking&quot; is 4chan's imageboard for food pictures and cooking recipes."
        }
        (
            "cm",
            "\"/cm/ - Cute/Male\" is 4chan's imageboard for posting pictures of cute anime males.",
        ) => {
            "&quot;/cm/ - Cute/Male&quot; is 4chan's imageboard for posting pictures of cute anime males."
        }
        (
            "co",
            "\"/co/ - Comics & Cartoons\" is 4chan's imageboard dedicated to the discussion of Western cartoons and comics.",
        ) => {
            "&quot;/co/ - Comics &amp; Cartoons&quot; is 4chan's imageboard dedicated to the discussion of Western cartoons and comics."
        }
        (
            "d",
            "\"/d/ - Hentai/Alternative\" is 4chan's imageboard for alternative hentai images.",
        ) => {
            "&quot;/d/ - Hentai/Alternative&quot; is 4chan's imageboard for alternative hentai images."
        }
        (
            "diy",
            "\"/diy/ - Do It Yourself\" is 4chan's imageboard for DIY/do it yourself projects, home improvement, and makers.",
        ) => {
            "&quot;/diy/ - Do It Yourself&quot; is 4chan's imageboard for DIY/do it yourself projects, home improvement, and makers."
        }
        ("e", "\"/e/ - Ecchi\" is 4chan's imageboard for suggestive (ecchi) hentai images.") => {
            "&quot;/e/ - Ecchi&quot; is 4chan's imageboard for suggestive (ecchi) hentai images."
        }
        ("f", "\"/f/ - Flash\" is 4chan's upload board for sharing Adobe Flash files (SWFs).") => {
            "&quot;/f/ - Flash&quot; is 4chan's upload board for sharing Adobe Flash files (SWFs)."
        }
        (
            "fa",
            "\"/fa/ - Fashion\" is 4chan's imageboard for images and discussion relating to fashion and apparel.",
        ) => {
            "&quot;/fa/ - Fashion&quot; is 4chan's imageboard for images and discussion relating to fashion and apparel."
        }
        (
            "fit",
            "\"/fit/ - Fitness\" is 4chan's imageboard for weightlifting, health, and fitness.",
        ) => {
            "&quot;/fit/ - Fitness&quot; is 4chan's imageboard for weightlifting, health, and fitness."
        }
        (
            "g",
            "\"/g/ - Technology\" is 4chan's imageboard for discussing computer hardware and software, programming, and general technology.",
        ) => {
            "&quot;/g/ - Technology&quot; is 4chan's imageboard for discussing computer hardware and software, programming, and general technology."
        }
        ("gd", "\"/gd/ - Graphic Design\" is 4chan's imageboard for graphic design.") => {
            "&quot;/gd/ - Graphic Design&quot; is 4chan's imageboard for graphic design."
        }
        (
            "gif",
            "\"/gif/ - Adult GIF\" is 4chan's imageboard dedicated to animated adult GIFs and WEBMs.",
        ) => {
            "&quot;/gif/ - Adult GIF&quot; is 4chan's imageboard dedicated to animated adult GIFs and WEBMs."
        }
        ("h", "\"/h/ - Hentai\" is 4chan's imageboard for adult Japanese anime hentai images.") => {
            "&quot;/h/ - Hentai&quot; is 4chan's imageboard for adult Japanese anime hentai images."
        }
        (
            "hc",
            "\"/hc/ - Hardcore\" is 4chan's imageboard for the posting of adult hardcore pornography.",
        ) => {
            "&quot;/hc/ - Hardcore&quot; is 4chan's imageboard for the posting of adult hardcore pornography."
        }
        (
            "his",
            "\"/his/ - History & Humanities\" is 4chan's board for discussing and debating history.",
        ) => {
            "&quot;/his/ - History &amp; Humanities&quot; is 4chan's board for discussing and debating history."
        }
        (
            "hm",
            "\"/hm/ - Handsome Men\" is 4chan's imageboard dedicated to sharing adult images of handsome men.",
        ) => {
            "&quot;/hm/ - Handsome Men&quot; is 4chan's imageboard dedicated to sharing adult images of handsome men."
        }
        (
            "hr",
            "\"/hr/ - High Resolution\" is 4chan's imageboard for the sharing of high resolution images.",
        ) => {
            "&quot;/hr/ - High Resolution&quot; is 4chan's imageboard for the sharing of high resolution images."
        }
        ("i", "\"/i/ - Oekaki\" is 4chan's oekaki board for drawing and sharing art.") => {
            "&quot;/i/ - Oekaki&quot; is 4chan's oekaki board for drawing and sharing art."
        }
        (
            "ic",
            "\"/ic/ - Artwork/Critique\" is 4chan's imageboard for the discussion and critique of art.",
        ) => {
            "&quot;/ic/ - Artwork/Critique&quot; is 4chan's imageboard for the discussion and critique of art."
        }
        (
            "int",
            "\"/int/ - International\" is 4chan's international board, for the exchange of foreign language and culture.",
        ) => {
            "&quot;/int/ - International&quot; is 4chan's international board, for the exchange of foreign language and culture."
        }
        (
            "jp",
            "\"/jp/ - Otaku Culture\" is 4chan's board for discussing Japanese otaku culture.",
        ) => {
            "&quot;/jp/ - Otaku Culture&quot; is 4chan's board for discussing Japanese otaku culture."
        }
        (
            "k",
            "\"/k/ - Weapons\" is 4chan's imageboard for discussing all types of weaponry, from military tanks to guns and knives.",
        ) => {
            "&quot;/k/ - Weapons&quot; is 4chan's imageboard for discussing all types of weaponry, from military tanks to guns and knives."
        }
        (
            "lgbt",
            "\"/lgbt/ - LGBT\" is 4chan's imageboard for Lesbian-Gay-Bisexual-Transgender-Queer and sexuality discussion.",
        ) => {
            "&quot;/lgbt/ - LGBT&quot; is 4chan's imageboard for Lesbian-Gay-Bisexual-Transgender-Queer and sexuality discussion."
        }
        (
            "lit",
            "\"/lit/ - Literature\" is 4chan's board for the discussion of books, authors, and literature.",
        ) => {
            "&quot;/lit/ - Literature&quot; is 4chan's board for the discussion of books, authors, and literature."
        }
        (
            "m",
            "\"/m/ - Mecha\" is 4chan's imageboard for discussing Japanese mecha robots and anime, like Gundam and Macross.",
        ) => {
            "&quot;/m/ - Mecha&quot; is 4chan's imageboard for discussing Japanese mecha robots and anime, like Gundam and Macross."
        }
        (
            "mlp",
            "\"/mlp/ - Pony\" is 4chan's imageboard dedicated to the discussion of My Little Pony: Friendship is Magic.",
        ) => {
            "&quot;/mlp/ - Pony&quot; is 4chan's imageboard dedicated to the discussion of My Little Pony: Friendship is Magic."
        }
        ("mu", "\"/mu/ - Music\" is 4chan's imageboard for discussing all types of music.") => {
            "&quot;/mu/ - Music&quot; is 4chan's imageboard for discussing all types of music."
        }
        (
            "n",
            "\"/n/ - Transportation\" is 4chan's imageboard for discussing modes of transportation like trains and bicycles.",
        ) => {
            "&quot;/n/ - Transportation&quot; is 4chan's imageboard for discussing modes of transportation like trains and bicycles."
        }
        ("news", "\"/news/ - Current News; is 4chan's board for current news.") => {
            "&quot;/news/ - Current News; is 4chan's board for current news. "
        }
        ("o", "\"/o/ - Auto\" is 4chan's imageboard for discussing cars and motorcycles.") => {
            "&quot;/o/ - Auto&quot; is 4chan's imageboard for discussing cars and motorcycles."
        }
        (
            "out",
            "\"/out/ - Outdoors\" is 4chan's imageboard for discussing survivalist skills and outdoor activities such as hiking.",
        ) => {
            "&quot;/out/ - Outdoors&quot; is 4chan's imageboard for discussing survivalist skills and outdoor activities such as hiking."
        }
        ("p", "\"/p/ - Photography\" is 4chan's imageboard for sharing and critiquing photos.") => {
            "&quot;/p/ - Photography&quot; is 4chan's imageboard for sharing and critiquing photos."
        }
        (
            "po",
            "\"/po/ - Papercraft & Origami\" is 4chan's imageboard for posting papercraft and origami templates and instructions.",
        ) => {
            "&quot;/po/ - Papercraft &amp; Origami&quot; is 4chan's imageboard for posting papercraft and origami templates and instructions."
        }
        (
            "pol",
            "\"/pol/ - Politically Incorrect\" is 4chan's board for discussing and debating politics and current events.",
        ) => {
            "&quot;/pol/ - Politically Incorrect&quot; is 4chan's board for discussing and debating politics and current events."
        }
        (
            "pw",
            "\"/pw/ - Professional Wrestling\" is 4chan's imageboard for the discussion of professional wrestling.",
        ) => {
            "&quot;/pw/ - Professional Wrestling&quot; is 4chan's imageboard for the discussion of professional wrestling."
        }
        (
            "qa",
            "\"/qa/ - Question & Answer\" is 4chan's imageboard for question and answer threads.",
        ) => {
            "&quot;/qa/ - Question &amp; Answer&quot; is 4chan's imageboard for question and answer threads."
        }
        ("qb", "\"/qb/ - Quebec\"") => "&quot;/qb/ - Quebec&quot;",
        ("qst", "\"/qst/ - Quests\" is 4chan's imageboard for grinding XP.") => {
            "&quot;/qst/ - Quests&quot; is 4chan's imageboard for grinding XP."
        }
        (
            "r",
            "\"/r/ - Request\" is 4chan's imageboard dedicated to fulfilling all types of user requests.",
        ) => {
            "&quot;/r/ - Request&quot; is 4chan's imageboard dedicated to fulfilling all types of user requests."
        }
        (
            "r9k",
            "\"/r9k/ - ROBOT9001\" is a board for hanging out and posting greentext stories.",
        ) => {
            "&quot;/r9k/ - ROBOT9001&quot; is a board for hanging out and posting greentext stories."
        }
        (
            "s",
            "\"/s/ - Sexy Beautiful Women\" is 4chan's imageboard dedicated to sharing images of softcore pornography.",
        ) => {
            "&quot;/s/ - Sexy Beautiful Women&quot; is 4chan's imageboard dedicated to sharing images of softcore pornography."
        }
        ("s4s", "\"[s4s] - Shit 4chan Says\" is 4chan's imageboard for posting dank memes :^)") => {
            "&quot;[s4s] - Shit 4chan Says&quot; is 4chan's imageboard for posting dank memes :^)"
        }
        (
            "sci",
            "\"/sci/ - Science & Math\" is 4chan's board for the discussion of science and math.",
        ) => {
            "&quot;/sci/ - Science &amp; Math&quot; is 4chan's board for the discussion of science and math."
        }
        ("soc", "\"/soc/ - Cams & Meetups\" is 4chan's board for camwhores and meetups.") => {
            "&quot;/soc/ - Cams &amp; Meetups&quot; is 4chan's board for camwhores and meetups."
        }
        ("sp", "\"/sp/ - Sports\" is 4chan's imageboard for sports discussion.") => {
            "&quot;/sp/ - Sports&quot; is 4chan's imageboard for sports discussion."
        }
        (
            "t",
            "\"/t/ - Torrents\" is 4chan's imageboard for posting links and descriptions to torrents.",
        ) => {
            "&quot;/t/ - Torrents&quot; is 4chan's imageboard for posting links and descriptions to torrents."
        }
        (
            "tg",
            "\"/tg/ - Traditional Games\" is 4chan's imageboard for discussing traditional gaming, such as board games and tabletop RPGs.",
        ) => {
            "&quot;/tg/ - Traditional Games&quot; is 4chan's imageboard for discussing traditional gaming, such as board games and tabletop RPGs."
        }
        ("toy", "\"/toy/ - Toys\" is 4chan's imageboard for talking about all kinds of toys!") => {
            "&quot;/toy/ - Toys&quot; is 4chan's imageboard for talking about all kinds of toys!"
        }
        ("trash", "\"/trash/ - Off-topic\" is 4chan's imageboard jail for off-topic threads.") => {
            "&quot;/trash/ - Off-topic&quot; is 4chan's imageboard jail for off-topic threads."
        }
        (
            "trv",
            "\"/trv/ - Travel\" is 4chan's imageboard dedicated to travel and the countries of the world.",
        ) => {
            "&quot;/trv/ - Travel&quot; is 4chan's imageboard dedicated to travel and the countries of the world."
        }
        (
            "tv",
            "\"/tv/ - Television & Film\" is 4chan's imageboard dedicated to the discussion of television and film.",
        ) => {
            "&quot;/tv/ - Television &amp; Film&quot; is 4chan's imageboard dedicated to the discussion of television and film."
        }
        ("u", "\"/u/ - Yuri\" is 4chan's imageboard for yuri hentai images.") => {
            "&quot;/u/ - Yuri&quot; is 4chan's imageboard for yuri hentai images."
        }
        (
            "v",
            "\"/v/ - Video Games\" is 4chan's imageboard dedicated to the discussion of PC and console video games.",
        ) => {
            "&quot;/v/ - Video Games&quot; is 4chan's imageboard dedicated to the discussion of PC and console video games."
        }
        (
            "vg",
            "\"/vg/ - Video Game Generals\" is 4chan's imageboard dedicated to the discussion of PC and console video games.",
        ) => {
            "&quot;/vg/ - Video Game Generals&quot; is 4chan's imageboard dedicated to the discussion of PC and console video games."
        }
        ("vip", "\"/vip/ - Very Important Posts\" is 4chan's imageboard for Pass users.") => {
            "&quot;/vip/ - Very Important Posts&quot; is 4chan's imageboard for Pass users."
        }
        (
            "vm",
            "\"/vmg/ - Video Games/Multiplayer\" is 4chan's imageboard dedicated to the discussion of multiplayer video games.",
        ) => {
            "&quot;/vmg/ - Video Games/Multiplayer&quot; is 4chan's imageboard dedicated to the discussion of multiplayer video games."
        }
        (
            "vmg",
            "\"/vmg/ - Video Games/Mobile\" is 4chan's imageboard dedicated to the discussion of video games on mobile devices.",
        ) => {
            "&quot;/vmg/ - Video Games/Mobile&quot; is 4chan's imageboard dedicated to the discussion of video games on mobile devices."
        }
        (
            "vp",
            "\"/vp/ - Pokémon\" is 4chan's imageboard dedicated to discussing the Pokémon series of video games and shows.",
        ) => {
            "&quot;/vp/ - Pok&eacute;mon&quot; is 4chan's imageboard dedicated to discussing the Pok&eacute;mon series of video games and shows."
        }
        (
            "vr",
            "\"/vr/ - Retro Games\" is 4chan's imageboard for discussing retro console video games and classic arcade games.",
        ) => {
            "&quot;/vr/ - Retro Games&quot; is 4chan's imageboard for discussing retro console video games and classic arcade games."
        }
        (
            "vrpg",
            "\"/vrpg/ - Video Games/RPG\" is 4chan's imageboard dedicated to the discussion of role-playing video games.",
        ) => {
            "&quot;/vrpg/ - Video Games/RPG&quot; is 4chan's imageboard dedicated to the discussion of role-playing video games."
        }
        (
            "vst",
            "\"/vst/ - Video Games/Strategy\" is 4chan's imageboard dedicated to the discussion of strategy video games.",
        ) => {
            "&quot;/vst/ - Video Games/Strategy&quot; is 4chan's imageboard dedicated to the discussion of strategy video games."
        }
        (
            "vt",
            "\"/vt/ - Virtual YouTubers\" is 4chan's board for discussing virtual YouTubers (\"VTubers\").",
        ) => {
            "&quot;/vt/ - Virtual YouTubers&quot; is 4chan's board for discussing virtual YouTubers (&quot;VTubers&quot;)."
        }
        (
            "w",
            "\"/w/ - Anime/Wallpapers\" is 4chan's imageboard for posting Japanese anime wallpapers.",
        ) => {
            "&quot;/w/ - Anime/Wallpapers&quot; is 4chan's imageboard for posting Japanese anime wallpapers."
        }
        (
            "wg",
            "\"/wg/ - Wallpapers/General\" is 4chan's imageboard for posting general wallpapers.",
        ) => {
            "&quot;/wg/ - Wallpapers/General&quot; is 4chan's imageboard for posting general wallpapers."
        }
        (
            "wsg",
            "\"/wsg/ - Worksafe GIF\" is 4chan's imageboard dedicated to sharing worksafe animated GIFs and WEBMs.",
        ) => {
            "&quot;/wsg/ - Worksafe GIF&quot; is 4chan's imageboard dedicated to sharing worksafe animated GIFs and WEBMs."
        }
        (
            "wsr",
            "\"/wsr/ - Worksafe Requests\" is 4chan's imageboard dedicated to fulfilling non-NSFW requests.",
        ) => {
            "&quot;/wsr/ - Worksafe Requests&quot; is 4chan's imageboard dedicated to fulfilling non-NSFW requests."
        }
        (
            "x",
            "\"/x/ - Paranormal\" is 4chan's imageboard for the discussion of paranormal, spooky pictures and conspiracy theories.",
        ) => {
            "&quot;/x/ - Paranormal&quot; is 4chan's imageboard for the discussion of paranormal, spooky pictures and conspiracy theories."
        }
        (
            "xs",
            "\"/xs/ - Extreme Sports\" is 4chan's imageboard imageboard dedicated to the discussion of extreme sports.",
        ) => {
            "&quot;/xs/ - Extreme Sports&quot; is 4chan's imageboard imageboard dedicated to the discussion of extreme sports."
        }
        ("y", "\"/y/ - Yaoi\" is 4chan's imageboard for posting adult yaoi hentai images.") => {
            "&quot;/y/ - Yaoi&quot; is 4chan's imageboard for posting adult yaoi hentai images."
        }
        _ => current,
    }
}
