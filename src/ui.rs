slint::slint! {
    import { ScrollView } from "std-widgets.slint";

    // ---- Charte R_M_X (skill appliquer-charte-rmx) ----
    // Dark-first 80/15/5 : fond #111111, surfaces #222222, texte #FFFFFF/#CCCCCC,
    // accents neons <= 5% (cyan #00FFFF actions/focus, magenta #FF00FF danger,
    // vert terminal #00FF7F pour le contexte terminal). Rayon unique 10px,
    // halo discret rgba(255,255,255,0.1), hover scale(1.05) en 0.2s ease
    // (seul mouvement autorise). HelpMe en display, monospace pour les donnees.
    // Maquette FigCraft : assets/ui-charte-rmx.figcraft.
    global RmxTheme {
        out property <color> fond: #111111;
        out property <color> surface: #222222;
        out property <color> input: #111111;
        out property <color> bordure: #333333;
        out property <color> bordure-claire: #444444;
        out property <color> texte: #FFFFFF;
        out property <color> texte-secondaire: #CCCCCC;
        out property <color> desactive: #555555;
        out property <color> cyan: #00FFFF;
        out property <color> magenta: #FF00FF;
        out property <color> vert-terminal: #00FF7F;
        out property <color> noir-terminal: #000000;
        out property <length> rayon: 10px;
    }

    // Icône vectorielle (paths Lucide 24x24, stroke uniquement, jamais d'emoji)
    component RmxIcon inherits Path {
        in property <string> kind: "";
        in property <color> couleur: RmxTheme.texte;
        width: 16px;
        height: 16px;
        viewbox-width: 24;
        viewbox-height: 24;
        fill: transparent;
        stroke: root.couleur;
        stroke-width: 2px;
        stroke-line-cap: round;
        stroke-line-join: round;
        commands: root.kind == "zap" ? "M13 2 L3 14 h7 l-1 8 L19 10 h-7 l1-8 Z"
            : root.kind == "x" ? "M18 6 L6 18 M6 6 l12 12"
            : root.kind == "search" ? "M18 11 a7 7 0 1 1 -14 0 a7 7 0 1 1 14 0 M21 21 l-4.35 -4.35"
            : root.kind == "folder" ? "M22 19 a2 2 0 0 1 -2 2 H4 a2 2 0 0 1 -2 -2 V5 a2 2 0 0 1 2 -2 h5 l2 3 h9 a2 2 0 0 1 2 2 Z"
            : root.kind == "repeat" ? "M17 2 l4 4 -4 4 M3 11 v-1 a4 4 0 0 1 4 -4 h14 M7 22 l-4 -4 4 -4 M21 13 v1 a4 4 0 0 1 -4 4 H3"
            : root.kind == "arrow-up-right" ? "M7 17 L17 7 M7 7 h10 v10"
            : root.kind == "check" ? "M20 6 L9 17 l-5 -5"
            : "M5 12 h14";
    }

    // Titre de section : HelpMe display, gris, petites capitales espacees
    component RmxSectionTitle inherits Text {
        color: RmxTheme.texte-secondaire;
        font-family: "HelpMe";
        font-size: 11px;
        font-weight: 700;
        letter-spacing: 2px;
        height: 16px;
    }

    // Carte : surface #222, rayon 10px, bordure #333, halo discret rgba(255,255,255,0.1)
    component RmxCard inherits Rectangle {
        background: transparent;
        Rectangle {
            x: -3px;
            y: -3px;
            width: parent.width + 6px;
            height: parent.height + 6px;
            border-radius: 13px;
            background: #FFFFFF;
            opacity: 0.05;
        }
        Rectangle {
            width: parent.width;
            height: parent.height;
            border-radius: RmxTheme.rayon;
            background: RmxTheme.surface;
            border-width: 1px;
            border-color: RmxTheme.bordure;
        }
    }

    // Carte terminal : fond noir pur, pour le journal (vert #00FF7F sur noir)
    component RmxTerminalCard inherits Rectangle {
        background: transparent;
        Rectangle {
            x: -3px;
            y: -3px;
            width: parent.width + 6px;
            height: parent.height + 6px;
            border-radius: 13px;
            background: #FFFFFF;
            opacity: 0.05;
        }
        Rectangle {
            width: parent.width;
            height: parent.height;
            border-radius: RmxTheme.rayon;
            background: RmxTheme.noir-terminal;
            border-width: 1px;
            border-color: RmxTheme.bordure;
        }
    }

    component RmxLabel inherits Text {
        in property <length> label-width: 104px;
        width: label-width;
        height: 32px;
        color: RmxTheme.texte;
        font-size: 13px;
        vertical-alignment: TextVerticalAlignment.center;
        overflow: elide;
    }

    component RmxValue inherits Rectangle {
        in property <string> text;
        background: RmxTheme.input;
        border-width: 1px;
        border-color: RmxTheme.bordure;
        border-radius: RmxTheme.rayon;
        height: 32px;
        min-width: 96px;
        Text {
            width: parent.width;
            height: parent.height;
            text: root.text;
            color: RmxTheme.texte;
            font-family: "monospace";
            font-size: 13px;
            horizontal-alignment: TextHorizontalAlignment.center;
            vertical-alignment: TextVerticalAlignment.center;
            overflow: elide;
        }
    }

    // Champ de saisie : fond #111, bordure #333, bordure cyan au focus (focus visible)
    component RmxField inherits Rectangle {
        in-out property <string> text;
        in property <string> placeholder;
        in property <bool> centered: false;
        callback edited(string);
        background: RmxTheme.input;
        border-width: 1px;
        border-color: input.has-focus ? RmxTheme.cyan : RmxTheme.bordure;
        border-radius: RmxTheme.rayon;
        height: 32px;
        min-width: 120px;
        forward-focus: input;
        input := TextInput {
            x: 12px;
            width: parent.width - 24px;
            height: parent.height;
            text <=> root.text;
            color: RmxTheme.texte;
            font-family: "monospace";
            font-size: 13px;
            horizontal-alignment: root.centered ? TextHorizontalAlignment.center : TextHorizontalAlignment.left;
            vertical-alignment: TextVerticalAlignment.center;
            edited => { root.edited(self.text); }
        }
        if input.text == "" && !input.has-focus : Text {
            x: 12px;
            width: parent.width - 24px;
            height: parent.height;
            text: root.placeholder;
            color: RmxTheme.texte-secondaire;
            font-family: "monospace";
            font-size: 12px;
            horizontal-alignment: root.centered ? TextHorizontalAlignment.center : TextHorizontalAlignment.left;
            vertical-alignment: TextVerticalAlignment.center;
            overflow: elide;
        }
    }

    component RmxButton inherits Rectangle {
        in property <string> text;
        in property <string> icon: "";
        in property <bool> primary: false;
        in property <bool> danger: false;
        in property <bool> enabled: true;
        callback clicked();
        property <bool> hovered;
        property <bool> pressed;
        height: 36px;
        min-width: 96px;
        border-radius: RmxTheme.rayon;
        border-width: 1px;
        background: !root.enabled ? transparent
            : root.primary ? RmxTheme.cyan
            : root.hovered ? #2A2A2A
            : transparent;
        border-color: !root.enabled ? RmxTheme.bordure
            : root.primary ? RmxTheme.cyan
            : root.danger ? RmxTheme.magenta
            : root.hovered ? RmxTheme.texte
            : RmxTheme.bordure-claire;
        // Halo discret sur le bouton primaire
        if root.primary && root.enabled : Rectangle {
            x: -3px;
            y: -3px;
            width: parent.width + 6px;
            height: parent.height + 6px;
            border-radius: 13px;
            background: #FFFFFF;
            opacity: 0.08;
        }
        // Seul effet de mouvement autorise : hover scale(1.05) en 0.2s ease
        transform-origin: { x: root.width / 2, y: root.height / 2 };
        transform-scale-x: root.hovered && root.enabled ? 105% : 100%;
        transform-scale-y: root.hovered && root.enabled ? 105% : 100%;
        animate transform-scale-x { duration: 200ms; easing: ease; }
        animate transform-scale-y { duration: 200ms; easing: ease; }
        HorizontalLayout {
            padding-left: 12px;
            padding-right: 12px;
            spacing: 6px;
            alignment: LayoutAlignment.center;
            cross-axis-alignment: CrossAxisAlignment.center;
            if root.icon != "" : RmxIcon {
                kind: root.icon;
                couleur: !root.enabled ? RmxTheme.desactive
                    : root.primary ? RmxTheme.fond
                    : root.danger ? RmxTheme.magenta
                    : RmxTheme.texte;
            }
            Text {
                text: root.text;
                color: !root.enabled ? RmxTheme.desactive
                    : root.primary ? RmxTheme.fond
                    : root.danger ? RmxTheme.magenta
                    : RmxTheme.texte;
                font-size: 13px;
                font-weight: root.primary ? 700 : 400;
                vertical-alignment: TextVerticalAlignment.center;
                overflow: elide;
            }
        }
        touch := TouchArea {
            visible: root.enabled;
            mouse-cursor: pointer;
            clicked => { root.clicked(); }
            changed has-hover => { root.hovered = self.has-hover; }
            pointer-event => { root.pressed = self.pressed; }
        }
    }

    component RmxCheckBox inherits Rectangle {
        in property <string> text;
        in-out property <bool> checked;
        callback toggled();
        property <bool> hovered;
        height: 24px;
        transform-origin: { x: root.width / 2, y: root.height / 2 };
        transform-scale-x: root.hovered ? 105% : 100%;
        transform-scale-y: root.hovered ? 105% : 100%;
        animate transform-scale-x { duration: 200ms; easing: ease; }
        animate transform-scale-y { duration: 200ms; easing: ease; }
        HorizontalLayout {
            padding-left: 2px;
            spacing: 8px;
            cross-axis-alignment: CrossAxisAlignment.center;
            Rectangle {
                width: 16px;
                height: 16px;
                border-radius: RmxTheme.rayon;
                border-width: 1px;
                border-color: root.checked ? RmxTheme.cyan : RmxTheme.texte-secondaire;
                background: root.checked ? RmxTheme.cyan : transparent;
                if root.checked : RmxIcon {
                    kind: "check";
                    couleur: RmxTheme.fond;
                    x: 2px;
                    y: 2px;
                    width: 12px;
                    height: 12px;
                }
            }
            Text {
                text: root.text;
                color: RmxTheme.texte;
                font-size: 12px;
                vertical-alignment: TextVerticalAlignment.center;
                overflow: elide;
            }
        }
        TouchArea {
            mouse-cursor: pointer;
            clicked => {
                root.checked = !root.checked;
                root.toggled();
            }
            changed has-hover => { root.hovered = self.has-hover; }
        }
    }

    // Slider : track gris, remplissage et boule cyan, rayon 10px
    component RmxSlider inherits Rectangle {
        in-out property <float> value;
        in property <float> min: 1;
        in property <float> max: 32;
        callback changed(float);
        height: 32px;
        property <float> ratio: Math.min(Math.max((root.value - root.min) / (root.max - root.min), 0), 1);
        Rectangle {
            y: parent.height / 2 - 3px;
            height: 6px;
            width: parent.width;
            border-radius: RmxTheme.rayon;
            background: RmxTheme.bordure-claire;
            Rectangle {
                width: parent.width * root.ratio;
                height: parent.height;
                border-radius: RmxTheme.rayon;
                background: RmxTheme.cyan;
            }
        }
        Rectangle {
            width: 18px;
            height: 18px;
            border-radius: 9px;
            background: RmxTheme.cyan;
            border-width: 2px;
            border-color: RmxTheme.fond;
            x: (parent.width - self.width) * root.ratio;
            y: parent.height / 2 - self.height / 2;
        }
        ta := TouchArea {
            mouse-cursor: pointer;
            property <float> r: Math.min(Math.max(self.mouse-x / root.width, 0), 1);
            clicked => {
                root.value = root.min + (root.max - root.min) * self.r;
                root.changed(root.value);
            }
            moved => {
                if (self.pressed) {
                    root.value = root.min + (root.max - root.min) * self.r;
                    root.changed(root.value);
                }
            }
        }
    }

    // Switcher segmente Envoyer / Recevoir : pill actif cyan, hover scale
    component RmxModeSwitch inherits Rectangle {
        in-out property <string> mode: "receive";
        callback mode-set(string);
        background: RmxTheme.surface;
        border-width: 1px;
        border-color: RmxTheme.bordure;
        border-radius: RmxTheme.rayon;
        height: 40px;
        width: 336px;
        HorizontalLayout {
            padding: 4px;
            spacing: 4px;
            Rectangle {
                border-radius: RmxTheme.rayon;
                background: root.mode == "send" ? RmxTheme.cyan : transparent;
                transform-origin: { x: self.width / 2, y: self.height / 2 };
                transform-scale-x: ta1.has-hover ? 105% : 100%;
                transform-scale-y: ta1.has-hover ? 105% : 100%;
                animate transform-scale-x { duration: 200ms; easing: ease; }
                animate transform-scale-y { duration: 200ms; easing: ease; }
                Text {
                    width: parent.width;
                    height: parent.height;
                    text: "Envoyer";
                    color: root.mode == "send" ? RmxTheme.fond : RmxTheme.texte;
                    font-size: 13px;
                    font-weight: root.mode == "send" ? 700 : 400;
                    horizontal-alignment: TextHorizontalAlignment.center;
                    vertical-alignment: TextVerticalAlignment.center;
                }
                ta1 := TouchArea {
                    mouse-cursor: pointer;
                    clicked => { root.mode-set("send"); }
                }
            }
            Rectangle {
                border-radius: RmxTheme.rayon;
                background: root.mode == "receive" ? RmxTheme.cyan : transparent;
                transform-origin: { x: self.width / 2, y: self.height / 2 };
                transform-scale-x: ta2.has-hover ? 105% : 100%;
                transform-scale-y: ta2.has-hover ? 105% : 100%;
                animate transform-scale-x { duration: 200ms; easing: ease; }
                animate transform-scale-y { duration: 200ms; easing: ease; }
                Text {
                    width: parent.width;
                    height: parent.height;
                    text: "Recevoir";
                    color: root.mode == "receive" ? RmxTheme.fond : RmxTheme.texte;
                    font-size: 13px;
                    font-weight: root.mode == "receive" ? 700 : 400;
                    horizontal-alignment: TextHorizontalAlignment.center;
                    vertical-alignment: TextVerticalAlignment.center;
                }
                ta2 := TouchArea {
                    mouse-cursor: pointer;
                    clicked => { root.mode-set("receive"); }
                }
            }
        }
    }

    // Barre de progression : track surface, remplissage cyan, rayon 10px
    component RmxProgress inherits Rectangle {
        in property <float> progress;
        in property <string> text;
        height: 36px;
        VerticalLayout {
            spacing: 4px;
            Rectangle {
                height: 8px;
                border-radius: RmxTheme.rayon;
                background: RmxTheme.surface;
                border-width: 1px;
                border-color: RmxTheme.bordure;
                Rectangle {
                    width: parent.width * Math.min(Math.max(root.progress, 0), 1);
                    height: parent.height;
                    border-radius: RmxTheme.rayon;
                    background: RmxTheme.cyan;
                }
            }
            Text {
                text: root.text;
                color: RmxTheme.texte-secondaire;
                font-family: "monospace";
                font-size: 11px;
                horizontal-alignment: TextHorizontalAlignment.center;
                overflow: elide;
            }
        }
    }

    export component AppWindow inherits Window {
        background: RmxTheme.fond;

        in-out property <string> mode: "receive";
        in-out property <bool> mode-send: false;
        in-out property <string> code;
        in-out property <string> dir;
        in-out property <string> addr: "0.0.0.0:47500";
        in-out property <string> out-dir;
        in-out property <bool> upnp: true;
        in-out property <bool> open-fw: true;
        in-out property <bool> close-fw: true;
        in-out property <bool> prefer-ipv6: false;
        in-out property <int> pipeline: 8;
        in-out property <float> pipeline-float: 8;
        in-out property <bool> running: false;
        in-out property <string> log-text;
        in-out property <string> pipeline-text: "8";
        in-out property <float> progress: 0;
        in-out property <string> progress-text: "";
        in-out property <bool> awaiting-approval: false;
        in-out property <string> manifest-text: "";
        in-out property <string> cmd-text: "";
        in-out property <string> exclude-text: "";
        in-out property <string> limit-text: "";
        in-out property <image> qr-image;
        in-out property <bool> qr-visible: false;
        in-out property <string> hidden: "TGUgdmVydCBkYW5zIGxlIG5vaXIsIGxlIHNvbiBkYW5zIGxlcyBmaWxzLg==";

        callback mode-set(string);
        callback browse-dir();
        callback browse-out-dir();
        callback generate-code();
        callback code-edited(string);
        callback start-transfer();
        callback cancel-transfer();
        callback dir-edited(string);
        callback accept-transfer();
        callback refuse-transfer();
        callback copy-command();
        callback discover();

        min-width: 680px;
        min-height: 720px;
        preferred-width: 840px;
        preferred-height: 780px;

        VerticalLayout {
            padding: 16px;
            spacing: 8px;

            // Header : H1 centre en HelpMe + curseur statique cyan + version mono + filet
            Rectangle {
                height: 44px;
                VerticalLayout {
                    spacing: 6px;
                    HorizontalLayout {
                        spacing: 4px;
                        alignment: LayoutAlignment.center;
                        Text {
                            text: "DirTransfert";
                            color: RmxTheme.texte;
                            font-family: "HelpMe";
                            font-size: 26px;
                            font-weight: 700;
                            vertical-alignment: TextVerticalAlignment.center;
                        }
                        Text {
                            text: "█";
                            color: RmxTheme.cyan;
                            font-family: "HelpMe";
                            font-size: 26px;
                            vertical-alignment: TextVerticalAlignment.center;
                        }
                    }
                    Rectangle {
                        height: 1px;
                        background: RmxTheme.bordure;
                    }
                }
                Text {
                    x: parent.width - 70px;
                    y: 14px;
                    width: 60px;
                    height: 16px;
                    text: "v0.8.0";
                    color: RmxTheme.texte-secondaire;
                    font-family: "monospace";
                    font-size: 10px;
                    horizontal-alignment: TextHorizontalAlignment.right;
                    vertical-alignment: TextVerticalAlignment.center;
                }
            }

            // Mode : switcher segmente
            RmxModeSwitch {
                mode <=> root.mode;
                mode-set(m) => { root.mode-set(m); }
            }

            // Card Connexion : code, adresse, commande, QR terminal
            RmxCard {
                HorizontalLayout {
                    padding: 12px;
                    spacing: 16px;
                    VerticalLayout {
                        spacing: 8px;
                        vertical-stretch: 1;
                        RmxSectionTitle { text: "CONNEXION"; }
                        HorizontalLayout {
                            spacing: 8px;
                            height: 32px;
                            RmxLabel { text: "Code :"; }
                            RmxField {
                                text <=> root.code;
                                placeholder: "XXXXX-XXXXX";
                                centered: true;
                                width: 240px;
                                edited(t) => { root.code-edited(t); }
                            }
                            RmxButton {
                                text: "Générer";
                                icon: "repeat";
                                min-width: 112px;
                                clicked => { root.generate-code(); }
                            }
                            Rectangle { }
                        }
                        HorizontalLayout {
                            spacing: 8px;
                            height: 32px;
                            RmxLabel { text: "Adresse :"; }
                            RmxField {
                                text <=> root.addr;
                                placeholder: "hote:port (ex. 192.168.1.20:47500) — ou Découvrir en mode Envoi";
                                centered: true;
                            }
                            Rectangle { }
                        }
                        if root.mode == "receive" && !root.running : HorizontalLayout {
                            spacing: 8px;
                            height: 24px;
                            RmxLabel {
                                text: "Commande :";
                                height: 24px;
                                color: RmxTheme.texte-secondaire;
                                font-family: "monospace";
                                font-size: 11px;
                            }
                            Text {
                                text: root.cmd-text;
                                color: RmxTheme.texte-secondaire;
                                font-family: "monospace";
                                font-size: 11px;
                                vertical-alignment: TextVerticalAlignment.center;
                                overflow: elide;
                            }
                        }
                    }
                    if root.mode == "receive" && root.qr-visible && !root.running : Rectangle {
                        width: 104px;
                        height: 104px;
                        background: RmxTheme.noir-terminal;
                        border-width: 1px;
                        border-color: RmxTheme.bordure;
                        border-radius: RmxTheme.rayon;
                        Image {
                            x: 4px;
                            y: 4px;
                            width: 96px;
                            height: 96px;
                            source: root.qr-image;
                            image-fit: contain;
                        }
                    }
                }
            }

            // Card Dossier
            RmxCard {
                VerticalLayout {
                    padding: 12px;
                    spacing: 8px;
                    RmxSectionTitle { text: "DOSSIER"; }
                    HorizontalLayout {
                        spacing: 8px;
                        height: 32px;
                        RmxLabel {
                            text: root.mode == "send" ? "Répertoire :" : "Réception :";
                            label-width: 120px;
                        }
                        RmxField {
                            text: root.mode == "send" ? root.dir : root.out-dir;
                            placeholder: "/chemin/vers/repertoire";
                            edited(t) => { root.dir-edited(t); }
                        }
                        RmxButton {
                            text: "Parcourir";
                            icon: "folder";
                            clicked => { root.mode == "send" ? root.browse-dir() : root.browse-out-dir(); }
                        }
                    }
                }
            }

            // Card Options
            RmxCard {
                VerticalLayout {
                    padding: 12px;
                    spacing: 8px;
                    RmxSectionTitle { text: "OPTIONS"; }
                    HorizontalLayout {
                        spacing: 24px;
                        height: 24px;
                        RmxCheckBox { text: "UPnP"; checked <=> root.upnp; }
                        RmxCheckBox { text: "Pare-feu"; checked <=> root.open-fw; }
                        RmxCheckBox { text: "IPv6"; checked <=> root.prefer-ipv6; }
                        RmxCheckBox { text: "Fermer après"; checked <=> root.close-fw; }
                    }
                    HorizontalLayout {
                        spacing: 8px;
                        height: 32px;
                        RmxLabel { text: "Exclure :"; label-width: 80px; }
                        RmxField {
                            text <=> root.exclude-text;
                            placeholder: ".git, node_modules, *.tmp";
                        }
                        RmxLabel { text: "Limite :"; label-width: 64px; }
                        RmxField {
                            text <=> root.limit-text;
                            placeholder: "Mo/s";
                            centered: true;
                            width: 88px;
                        }
                    }
                    HorizontalLayout {
                        spacing: 8px;
                        height: 32px;
                        RmxLabel { text: "Pipeline :"; label-width: 80px; }
                        RmxSlider {
                            value <=> root.pipeline-float;
                            changed(v) => {
                                root.pipeline-text = "N=" + floor(v);
                                root.pipeline = round(v);
                            }
                        }
                        RmxValue { text: root.pipeline-text; }
                    }
                }
            }

            // Progression
            RmxProgress {
                progress: root.progress;
                text: root.progress-text;
            }

            // Approbation du manifeste (mode réception)
            if root.awaiting-approval : RmxCard {
                VerticalLayout {
                    padding: 12px;
                    spacing: 8px;
                    RmxSectionTitle { text: "TRANSFERT ENTRANT"; }
                    Text {
                        text: root.manifest-text;
                        color: RmxTheme.texte;
                        font-size: 12px;
                        wrap: word-wrap;
                    }
                    HorizontalLayout {
                        spacing: 8px;
                        RmxButton {
                            text: "Accepter";
                            primary: true;
                            clicked => { root.accept-transfer(); }
                        }
                        RmxButton {
                            text: "Refuser";
                            danger: true;
                            clicked => { root.refuse-transfer(); }
                        }
                    }
                }
            }

            // Actions : neon reserve aux actions (primaire cyan, danger magenta)
            HorizontalLayout {
                spacing: 8px;
                height: 36px;
                RmxButton {
                    text: "Démarrer";
                    icon: "zap";
                    primary: true;
                    min-width: 128px;
                    enabled: !root.running && !root.awaiting-approval;
                    clicked => { root.start-transfer(); }
                }
                RmxButton {
                    text: "Annuler";
                    icon: "x";
                    danger: true;
                    min-width: 104px;
                    enabled: root.running;
                    clicked => { root.cancel-transfer(); }
                }
                cmd-field := TextInput {
                    visible: false;
                    width: 0;
                    height: 0;
                    read-only: true;
                    text: root.cmd-text;
                }
                if root.mode == "receive" : RmxButton {
                    text: "Copier la commande";
                    icon: "arrow-up-right";
                    enabled: !root.running;
                    clicked => {
                        root.copy-command();
                        cmd-field.select-all();
                        cmd-field.copy();
                    }
                }
                if root.mode == "send" : RmxButton {
                    text: "Découvrir";
                    icon: "search";
                    enabled: !root.running;
                    clicked => { root.discover(); }
                }
                Rectangle { }
            }

            // Journal : contexte terminal, vert #00FF7F sur noir
            RmxTerminalCard {
                vertical-stretch: 1;
                min-height: 80px;
                VerticalLayout {
                    padding: 12px;
                    spacing: 4px;
                    RmxSectionTitle { text: "JOURNAL"; font-size: 10px; }
                    log-scroll := ScrollView {
                        vertical-stretch: 1;
                        content-height: max(self.visible-height, log-input.preferred-height);
                        log-input := TextInput {
                            read-only: true;
                            single-line: false;
                            wrap: word-wrap;
                            text: root.log-text;
                            color: RmxTheme.vert-terminal;
                            font-family: "monospace";
                            font-size: 11px;
                        }
                    }
                }
            }

            // Footer / hidden layer (couche cachee : jamais expliquee dans l'interface)
            Rectangle {
                height: 18px;
                Text {
                    text: root.hidden;
                    color: #2A2A2A;
                    font-family: "monospace";
                    font-size: 9px;
                    horizontal-alignment: TextHorizontalAlignment.center;
                }
                Rectangle {
                    y: 16px;
                    height: 1px;
                    background: RmxTheme.surface;
                }
            }
        }
    }
}
