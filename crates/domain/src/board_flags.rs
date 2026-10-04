//! Source display labels and menu labels have separate meanings.
#[derive(Clone, Copy, Debug)]
pub struct Flag {
    pub code: &'static str,
    pub display: &'static str,
    pub selector: &'static str,
}
const POL: &[Flag] = &[
    Flag {
        code: "AC",
        display: "Anarcho-Capitalist",
        selector: "Anarcho-Capitalist",
    },
    Flag {
        code: "AN",
        display: "Anarchist",
        selector: "Anarchist",
    },
    Flag {
        code: "BL",
        display: "Black Lives Matter",
        selector: "Black Nationalist",
    },
    Flag {
        code: "CF",
        display: "Confederate",
        selector: "Confederate",
    },
    Flag {
        code: "CM",
        display: "Commie",
        selector: "Communist",
    },
    Flag {
        code: "CT",
        display: "Catalonia",
        selector: "Catalonia",
    },
    Flag {
        code: "DM",
        display: "Democrat",
        selector: "Democrat",
    },
    Flag {
        code: "EU",
        display: "European",
        selector: "European",
    },
    Flag {
        code: "FC",
        display: "Fascist",
        selector: "Fascist",
    },
    Flag {
        code: "GN",
        display: "Gadsden",
        selector: "Gadsden",
    },
    Flag {
        code: "GY",
        display: "LGBT",
        selector: "Gay",
    },
    Flag {
        code: "JH",
        display: "Jihadi",
        selector: "Jihadi",
    },
    Flag {
        code: "KN",
        display: "Kekistani",
        selector: "Kekistani",
    },
    Flag {
        code: "MF",
        display: "Muslim",
        selector: "Muslim",
    },
    Flag {
        code: "NB",
        display: "National Bolshevik",
        selector: "National Bolshevik",
    },
    Flag {
        code: "NT",
        display: "NATO",
        selector: "NATO",
    },
    Flag {
        code: "NZ",
        display: "Nazi",
        selector: "Nazi",
    },
    Flag {
        code: "PC",
        display: "Hippie",
        selector: "Hippie",
    },
    Flag {
        code: "PR",
        display: "Pirate",
        selector: "Pirate",
    },
    Flag {
        code: "RE",
        display: "Republican",
        selector: "Republican",
    },
    Flag {
        code: "MZ",
        display: "Task Force Z",
        selector: "Task Force Z",
    },
    Flag {
        code: "TM",
        display: "DEUS VULT",
        selector: "Templar",
    },
    Flag {
        code: "TR",
        display: "Tree Hugger",
        selector: "Tree Hugger",
    },
    Flag {
        code: "UN",
        display: "United Nations",
        selector: "United Nations",
    },
    Flag {
        code: "WP",
        display: "White Supremacist",
        selector: "White Supremacist",
    },
];
const MLP: &[Flag] = &[
    Flag {
        code: "4CC",
        display: "4cc /mlp/",
        selector: "4cc /mlp/",
    },
    Flag {
        code: "ADA",
        display: "Adagio Dazzle",
        selector: "Adagio Dazzle",
    },
    Flag {
        code: "AN",
        display: "Anon",
        selector: "Anon",
    },
    Flag {
        code: "ANF",
        display: "Anonfilly",
        selector: "Anonfilly",
    },
    Flag {
        code: "APB",
        display: "Apple Bloom",
        selector: "Apple Bloom",
    },
    Flag {
        code: "AJ",
        display: "Applejack",
        selector: "Applejack",
    },
    Flag {
        code: "AB",
        display: "Aria Blaze",
        selector: "Aria Blaze",
    },
    Flag {
        code: "AU",
        display: "Autumn Blaze",
        selector: "Autumn Blaze",
    },
    Flag {
        code: "BB",
        display: "Bon Bon",
        selector: "Bon Bon",
    },
    Flag {
        code: "BM",
        display: "Big Mac",
        selector: "Big Mac",
    },
    Flag {
        code: "BP",
        display: "Berry Punch",
        selector: "Berry Punch",
    },
    Flag {
        code: "BS",
        display: "Babs Seed",
        selector: "Babs Seed",
    },
    Flag {
        code: "CL",
        display: "Changeling",
        selector: "Changeling",
    },
    Flag {
        code: "CO",
        display: "Coco Pommel",
        selector: "Coco Pommel",
    },
    Flag {
        code: "CG",
        display: "Cozy Glow",
        selector: "Cozy Glow",
    },
    Flag {
        code: "CHE",
        display: "Cheerilee",
        selector: "Cheerilee",
    },
    Flag {
        code: "CB",
        display: "Cherry Berry",
        selector: "Cherry Berry",
    },
    Flag {
        code: "DAY",
        display: "Daybreaker",
        selector: "Daybreaker",
    },
    Flag {
        code: "DD",
        display: "Daring Do",
        selector: "Daring Do",
    },
    Flag {
        code: "DER",
        display: "Derpy Hooves",
        selector: "Derpy Hooves",
    },
    Flag {
        code: "DT",
        display: "Diamond Tiara",
        selector: "Diamond Tiara",
    },
    Flag {
        code: "DIS",
        display: "Discord",
        selector: "Discord",
    },
    Flag {
        code: "EQA",
        display: "EqG Applejack",
        selector: "EqG Applejack",
    },
    Flag {
        code: "EQF",
        display: "EqG Fluttershy",
        selector: "EqG Fluttershy",
    },
    Flag {
        code: "EQP",
        display: "EqG Pinkie Pie",
        selector: "EqG Pinkie Pie",
    },
    Flag {
        code: "EQR",
        display: "EqG Rainbow Dash",
        selector: "EqG Rainbow Dash",
    },
    Flag {
        code: "EQT",
        display: "EqG Trixie",
        selector: "EqG Trixie",
    },
    Flag {
        code: "EQI",
        display: "EqG Twilight Sparkle",
        selector: "EqG Twilight Sparkle",
    },
    Flag {
        code: "EQS",
        display: "EqG Sunset Shimmer",
        selector: "EqG Sunset Shimmer",
    },
    Flag {
        code: "ERA",
        display: "EqG Rarity",
        selector: "EqG Rarity",
    },
    Flag {
        code: "FAU",
        display: "Fausticorn",
        selector: "Fausticorn",
    },
    Flag {
        code: "FLE",
        display: "Fleur de lis",
        selector: "Fleur de lis",
    },
    Flag {
        code: "FL",
        display: "Fluttershy",
        selector: "Fluttershy",
    },
    Flag {
        code: "GI",
        display: "Gilda",
        selector: "Gilda",
    },
    Flag {
        code: "HT",
        display: "Hitch Trailblazer",
        selector: "Hitch Trailblazer",
    },
    Flag {
        code: "IZ",
        display: "Izzy Moonbow",
        selector: "Izzy Moonbow",
    },
    Flag {
        code: "LI",
        display: "Limestone",
        selector: "Limestone",
    },
    Flag {
        code: "LT",
        display: "Lord Tirek",
        selector: "Lord Tirek",
    },
    Flag {
        code: "LY",
        display: "Lyra Heartstrings",
        selector: "Lyra Heartstrings",
    },
    Flag {
        code: "MA",
        display: "Marble",
        selector: "Marble",
    },
    Flag {
        code: "MAU",
        display: "Maud",
        selector: "Maud",
    },
    Flag {
        code: "MIN",
        display: "Minuette",
        selector: "Minuette",
    },
    Flag {
        code: "NI",
        display: "Nightmare Moon",
        selector: "Nightmare Moon",
    },
    Flag {
        code: "NUR",
        display: "Nurse Redheart",
        selector: "Nurse Redheart",
    },
    Flag {
        code: "OCT",
        display: "Octavia",
        selector: "Octavia",
    },
    Flag {
        code: "PAR",
        display: "Parasprite",
        selector: "Parasprite",
    },
    Flag {
        code: "PC",
        display: "Princess Cadance",
        selector: "Princess Cadance",
    },
    Flag {
        code: "PCE",
        display: "Princess Celestia",
        selector: "Princess Celestia",
    },
    Flag {
        code: "PI",
        display: "Pinkie Pie",
        selector: "Pinkie Pie",
    },
    Flag {
        code: "PLU",
        display: "Princess Luna",
        selector: "Princess Luna",
    },
    Flag {
        code: "PM",
        display: "Pinkamena",
        selector: "Pinkamena",
    },
    Flag {
        code: "PP",
        display: "Pipp Petals",
        selector: "Pipp Petals",
    },
    Flag {
        code: "QC",
        display: "Queen Chrysalis",
        selector: "Queen Chrysalis",
    },
    Flag {
        code: "RAR",
        display: "Rarity",
        selector: "Rarity",
    },
    Flag {
        code: "RD",
        display: "Rainbow Dash",
        selector: "Rainbow Dash",
    },
    Flag {
        code: "RLU",
        display: "Roseluck",
        selector: "Roseluck",
    },
    Flag {
        code: "S1L",
        display: "S1 Luna",
        selector: "S1 Luna",
    },
    Flag {
        code: "SCO",
        display: "Scootaloo",
        selector: "Scootaloo",
    },
    Flag {
        code: "SHI",
        display: "Shining Armor",
        selector: "Shining Armor",
    },
    Flag {
        code: "SIL",
        display: "Silver Spoon",
        selector: "Silver Spoon",
    },
    Flag {
        code: "SON",
        display: "Sonata Dusk",
        selector: "Sonata Dusk",
    },
    Flag {
        code: "SP",
        display: "Spike",
        selector: "Spike",
    },
    Flag {
        code: "SPI",
        display: "Spitfire",
        selector: "Spitfire",
    },
    Flag {
        code: "SS",
        display: "Sunny Starscout",
        selector: "Sunny Starscout",
    },
    Flag {
        code: "STA",
        display: "Star Dancer",
        selector: "Star Dancer",
    },
    Flag {
        code: "STL",
        display: "Starlight Glimmer",
        selector: "Starlight Glimmer",
    },
    Flag {
        code: "SPT",
        display: "Sprout",
        selector: "Sprout",
    },
    Flag {
        code: "SUN",
        display: "Sunburst",
        selector: "Sunburst",
    },
    Flag {
        code: "SUS",
        display: "Sunset Shimmer",
        selector: "Sunset Shimmer",
    },
    Flag {
        code: "SWB",
        display: "Sweetie Belle",
        selector: "Sweetie Belle",
    },
    Flag {
        code: "TFA",
        display: "TFH Arizona",
        selector: "TFH Arizona",
    },
    Flag {
        code: "TFO",
        display: "TFH Oleander",
        selector: "TFH Oleander",
    },
    Flag {
        code: "TFP",
        display: "TFH Paprika",
        selector: "TFH Paprika",
    },
    Flag {
        code: "TFS",
        display: "TFH Shanty",
        selector: "TFH Shanty",
    },
    Flag {
        code: "TFT",
        display: "TFH Tianhuo",
        selector: "TFH Tianhuo",
    },
    Flag {
        code: "TFV",
        display: "TFH Velvet",
        selector: "TFH Velvet",
    },
    Flag {
        code: "TP",
        display: "TFH Pom",
        selector: "TFH Pom",
    },
    Flag {
        code: "TS",
        display: "Tempest Shadow",
        selector: "Tempest Shadow",
    },
    Flag {
        code: "TWI",
        display: "Twilight Sparkle",
        selector: "Twilight Sparkle",
    },
    Flag {
        code: "TX",
        display: "Trixie",
        selector: "Trixie",
    },
    Flag {
        code: "VS",
        display: "Vinyl Scratch",
        selector: "Vinyl Scratch",
    },
    Flag {
        code: "ZE",
        display: "Zecora",
        selector: "Zecora",
    },
    Flag {
        code: "ZS",
        display: "Zipp Storm",
        selector: "Zipp Storm",
    },
];
const LGBT: &[Flag] = &[
    Flag {
        code: "AAP",
        display: "AAP",
        selector: "AAP",
    },
    Flag {
        code: "ACE",
        display: "Asexual",
        selector: "Asexual",
    },
    Flag {
        code: "ACH",
        display: "Achillean",
        selector: "Achillean",
    },
    Flag {
        code: "AFB",
        display: "AFAB",
        selector: "AFAB",
    },
    Flag {
        code: "AGP",
        display: "AGP",
        selector: "AGP",
    },
    Flag {
        code: "AGR",
        display: "Agender",
        selector: "Agender",
    },
    Flag {
        code: "ALL",
        display: "LGBT",
        selector: "LGBT",
    },
    Flag {
        code: "ALY",
        display: "Ally",
        selector: "Ally",
    },
    Flag {
        code: "AMB",
        display: "AMAB",
        selector: "AMAB",
    },
    Flag {
        code: "AND",
        display: "Androgynous",
        selector: "Androgynous",
    },
    Flag {
        code: "ARO",
        display: "Aromantic",
        selector: "Aromantic",
    },
    Flag {
        code: "BCH",
        display: "Butch",
        selector: "Butch",
    },
    Flag {
        code: "BI",
        display: "Bisexual",
        selector: "Bisexual",
    },
    Flag {
        code: "BOY",
        display: "Boymoder",
        selector: "Boymoder",
    },
    Flag {
        code: "BR",
        display: "Bear",
        selector: "Bear",
    },
    Flag {
        code: "CHR",
        display: "Chaser",
        selector: "Chaser",
    },
    Flag {
        code: "CIS",
        display: "Cis",
        selector: "Cis",
    },
    Flag {
        code: "DOM",
        display: "Dom",
        selector: "Dom",
    },
    Flag {
        code: "DRO",
        display: "Demiromantic",
        selector: "Demiromantic",
    },
    Flag {
        code: "DSX",
        display: "Demisexual",
        selector: "Demisexual",
    },
    Flag {
        code: "FBY",
        display: "Femboy",
        selector: "Femboy",
    },
    Flag {
        code: "FFB",
        display: "FtM Femboy",
        selector: "FtM Femboy",
    },
    Flag {
        code: "FR",
        display: "FtM Repressor",
        selector: "FtM Repressor",
    },
    Flag {
        code: "GAY",
        display: "Gay",
        selector: "Gay",
    },
    Flag {
        code: "GFL",
        display: "Genderfluid",
        selector: "Genderfluid",
    },
    Flag {
        code: "GQR",
        display: "Genderqueer",
        selector: "Genderqueer",
    },
    Flag {
        code: "HFB",
        display: "HRT Femboy",
        selector: "HRT Femboy",
    },
    Flag {
        code: "HON",
        display: "Hon",
        selector: "Hon",
    },
    Flag {
        code: "HST",
        display: "HSTS",
        selector: "HSTS",
    },
    Flag {
        code: "INT",
        display: "Intersex",
        selector: "Intersex",
    },
    Flag {
        code: "LAB",
        display: "Labrys",
        selector: "Labrys",
    },
    Flag {
        code: "LES",
        display: "Lesbian",
        selector: "Lesbian",
    },
    Flag {
        code: "MBT",
        display: "MtF Butch",
        selector: "MtF Butch",
    },
    Flag {
        code: "MR",
        display: "MtF Repressor",
        selector: "MtF Repressor",
    },
    Flag {
        code: "NB",
        display: "Nonbinary",
        selector: "Nonbinary",
    },
    Flag {
        code: "OG",
        display: "Original",
        selector: "Original",
    },
    Flag {
        code: "PAN",
        display: "Pansexual",
        selector: "Pansexual",
    },
    Flag {
        code: "PBI",
        display: "Prison Bi",
        selector: "Prison Bi",
    },
    Flag {
        code: "PG",
        display: "Prison Gay",
        selector: "Prison Gay",
    },
    Flag {
        code: "PLY",
        display: "Poly",
        selector: "Poly",
    },
    Flag {
        code: "PNR",
        display: "Pooner",
        selector: "Pooner",
    },
    Flag {
        code: "PRG",
        display: "Progress",
        selector: "Progress",
    },
    Flag {
        code: "QES",
        display: "Questioning",
        selector: "Questioning",
    },
    Flag {
        code: "QR",
        display: "Queer",
        selector: "Queer",
    },
    Flag {
        code: "REP",
        display: "Repressor",
        selector: "Repressor",
    },
    Flag {
        code: "SPH",
        display: "Sapphic",
        selector: "Sapphic",
    },
    Flag {
        code: "STR",
        display: "Straight",
        selector: "Straight",
    },
    Flag {
        code: "SUB",
        display: "Sub",
        selector: "Sub",
    },
    Flag {
        code: "SW",
        display: "Switch",
        selector: "Switch",
    },
    Flag {
        code: "TF",
        display: "Transfem",
        selector: "Transfem",
    },
    Flag {
        code: "TKH",
        display: "Twinkhon",
        selector: "Twinkhon",
    },
    Flag {
        code: "TMA",
        display: "Transmasc",
        selector: "Transmasc",
    },
    Flag {
        code: "TNK",
        display: "Twink",
        selector: "Twink",
    },
    Flag {
        code: "TRN",
        display: "Transgender",
        selector: "Transgender",
    },
    Flag {
        code: "UKR",
        display: "Woke",
        selector: "Woke",
    },
];
const TEST: &[Flag] = &[
    Flag {
        code: "FL1",
        display: "Flag 1",
        selector: "Flag 1",
    },
    Flag {
        code: "FL2",
        display: "Flag 2",
        selector: "Flag 2",
    },
];
pub fn flags(kind: &str) -> &'static [Flag] {
    match kind {
        "pol" => POL,
        "mlp" => MLP,
        "lgbt" => LGBT,
        "test" => TEST,
        _ => &[],
    }
}
pub fn flag(kind: &str, code: &str) -> Option<Flag> {
    flags(kind).iter().copied().find(|flag| flag.code == code)
}
