slint::slint! {
    import { Button, CheckBox, LineEdit, ScrollView, Slider, ProgressIndicator } from "std-widgets.slint";

    // ---- Charte Terminal R_M_X v2 ----
    // Fond noir-vert #0A0F0A, unique teinte vert CRT #33FF66, halo CRT,
    // police HelpMe plein ecran, curseur bloc clignotant. Un seul ecran, pas de chrome.
    global RmxTheme {
        out property <color> fond: #0A0F0A;
        out property <color> crt: #33FF66;
        out property <color> crt-faible: #1a8033;
        out property <color> crt-voile: #12240f;
        out property <length> halo: 6px;
    }

    component RmxLabel inherits Rectangle {
        in property <string> text;
        in property <length> min-label-width: 90px;
        in property <bool> highlight: false;
        in property <TextHorizontalAlignment> h-alignment: left;
        height: 30px;
        min-width: min-label-width;
        Rectangle {
            width: parent.width;
            Text {
                width: parent.width;
                text: root.text;
                color: RmxTheme.crt;
                font-family: "HelpMe";
                font-size: 14px;
                vertical-alignment: center;
                horizontal-alignment: root.h-alignment;
                overflow: elide;
            }
        }
    }

    component RmxValue inherits Rectangle {
        in property <string> text;
        background: RmxTheme.crt-voile;
        height: 32px;
        min-width: 120px;
        Rectangle {
            width: parent.width;
            Text {
                width: parent.width;
                text: root.text;
                color: RmxTheme.crt;
                font-family: "HelpMe";
                font-size: 14px;
                vertical-alignment: center;
                horizontal-alignment: center;
                overflow: elide;
            }
        }
    }

    component RmxTextInput inherits Rectangle {
        in-out property <string> text;
        in property <string> placeholder;
        in property <TextHorizontalAlignment> h-alignment: left;
        callback edited(string);
        background: RmxTheme.crt-voile;
        height: 32px;
        min-width: 120px;
        forward-focus: input;
        TextInput {
            input := TextInput {
                width: parent.width;
                text <=> root.text;
                color: RmxTheme.crt;
                font-family: "HelpMe";
                font-size: 14px;
                horizontal-alignment: root.h-alignment;
                vertical-alignment: center;
                edited => { root.edited(self.text); }
            }
        }
        if (input.text == "" && !input.has-focus) : Text {
            x: 10px;
            width: parent.width - 20px;
            text: root.placeholder;
            color: RmxTheme.crt-faible;
            font-family: "HelpMe";
            font-size: 14px;
            horizontal-alignment: root.h-alignment;
            vertical-alignment: center;
        }
    }

    component RmxButton inherits Rectangle {
        in property <string> text;
        in property <bool> accent: false;
        in property <bool> primary: false;
        in property <bool> enabled: true;
        in property <int> font-weight: 400;
        in property <length> font-size: 14px;
        in property <length> min-button-width: 90px;
        callback clicked();
        property <bool> hovered;
        property <bool> pressed;
        property <bool> down: pressed && hovered;
        background: !root.enabled ? RmxTheme.crt-voile
            : root.primary ? RmxTheme.crt
            : (root.hovered ? RmxTheme.crt-voile : transparent);
        border-width: 1px;
        height: 34px;
        min-width: root.min-button-width;
        border-color: root.enabled ? RmxTheme.crt : RmxTheme.crt-faible;
        label := Text {
            text: root.text;
            color: !root.enabled ? RmxTheme.crt-faible : (root.primary ? RmxTheme.fond : RmxTheme.crt);
            font-family: "HelpMe";
            font-weight: root.font-weight;
            font-size: root.font-size;
            horizontal-alignment: center;
            vertical-alignment: center;
        }
        touch := TouchArea {
            visible: root.enabled;
            mouse-cursor: pointer;
            clicked => { root.clicked(); }
            changed has-hover => { root.hovered = self.has-hover; }
            pointer-event => {
                root.pressed = self.pressed;
            }
        }
    }

    component RmxCheckBox inherits Rectangle {
        in property <string> text;
        in-out property <bool> checked;
        callback toggled();
        property <bool> hovered;
        height: 28px;
        HorizontalLayout {
            padding-left: 2px;
            spacing: 8px;
            box-wrapper := Rectangle {
                width: 18px;
                box := Rectangle {
                    y: (parent.height - self.height) / 2;
                    width: 18px;
                    height: 18px;
                    border-width: 1px;
                    border-color: root.checked ? RmxTheme.crt : RmxTheme.crt-faible;
                    background: root.checked ? RmxTheme.crt : transparent;
                    if root.checked: Text {
                        text: "X";
                        color: RmxTheme.fond;
                        font-size: 14px;
                        horizontal-alignment: center;
                        vertical-alignment: center;
                    }
                }
                TouchArea {
                    mouse-cursor: pointer;
                    clicked => {
                        root.checked = !root.checked;
                        root.toggled();
                    }
                    changed has-hover => {
                        root.hovered = self.has-hover;
                    }
                }
            }
            Text {
                text: root.text;
                color: RmxTheme.crt;
                font-family: "HelpMe";
                font-size: 14px;
                vertical-alignment: center;
                overflow: elide;
            }
        }
    }

    component RmxSlider inherits Rectangle {
        in-out property <float> value;
        callback changed(float);
        height: 28px;
        HorizontalLayout {
            Slider {
                value <=> root.value;
                changed(value) => { root.changed(self.value); }
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
        min-height: 640px;
        preferred-width: 820px;
        preferred-height: 700px;

        VerticalLayout {
            padding: 16px;
            spacing: 10px;

                // Header
                Rectangle {
                    height: 58px;
                    Text {
                        y: 0px;
                        height: 30px;
                        text: "DirTransfert";
                        color: RmxTheme.crt;
                        font-family: "HelpMe";
                        font-size: 24px;
                        font-weight: 700;
                        horizontal-alignment: center;
                        vertical-alignment: top;
                    }
                    Rectangle {
                        y: 46px;
                        height: 1px;
                        background: RmxTheme.crt-faible;
                    }
                    cursor := Text {
                        text: "█";
                        color: RmxTheme.crt;
                        font-family: "HelpMe";
                        font-size: 24px;
                        x: parent.width / 2 + 92px;
                        y: 0px;
                        height: 30px;
                        opacity: 1;
                        animate opacity { duration: 1s; iteration-count: -1; direction: alternate; }
                    }
                }

                // Mode selection
                Rectangle {
                    height: 36px;
                    HorizontalLayout {
                        spacing: 10px;
                        RmxLabel { text: "Mode :"; }
                        RmxButton {
                            text: "Envoyer";
                            accent: root.mode == "send";
                            clicked => { mode-set("send"); }
                        }
                        RmxButton {
                            text: "Recevoir";
                            accent: root.mode == "receive";
                            clicked => { mode-set("receive"); }
                        }
                        Rectangle { }
                    }
                }

                // Code section
                Rectangle {
                    height: 36px;
                    HorizontalLayout {
                        spacing: 10px;
                        RmxLabel { text: "Code :"; }
                        code-input := RmxTextInput {
                            text <=> root.code;
                            placeholder: "XXXXX-XXXXX";
                            h-alignment: center;
                            edited(text) => { root.code-edited(text); }
                        }
                        RmxButton {
                            text: "Générer";
                            clicked => { root.generate-code(); }
                        }
                        Rectangle { }
                    }
                }

                // Directory section
                Rectangle {
                    height: 36px;
                    HorizontalLayout {
                        spacing: 10px;
                        RmxLabel { text: root.mode == "send" ? "Répertoire :" : "Réception :"; min-label-width: 110px; }
                        RmxTextInput {
                            text: root.mode == "send" ? root.dir : root.out-dir;
                            placeholder: "/chemin/vers/repertoire";
                            edited(text) => { root.dir-edited(text); }
                        }
                        RmxButton {
                            text: "Parcourir";
                            clicked => { root.mode == "send" ? root.browse-dir() : root.browse-out-dir(); }
                        }
                        Rectangle { }
                    }
                }

                // Address section
                Rectangle {
                    height: 36px;
                    HorizontalLayout {
                        spacing: 10px;
                        RmxLabel { text: "Adresse :"; }
                        RmxTextInput {
                            text <=> root.addr;
                            placeholder: "hote:port (ex. 192.168.1.20:47500) — ou Découvrir en mode Envoi";
                            h-alignment: center;
                        }
                        Rectangle { }
                    }
                }

                // Options
                Rectangle {
                    height: 36px;
                    HorizontalLayout {
                        spacing: 20px;
                        RmxCheckBox {
                            text: "UPnP";
                            checked <=> root.upnp;
                        }
                        RmxCheckBox {
                            text: "Pare-feu";
                            checked <=> root.open-fw;
                        }
                        RmxCheckBox {
                            text: "IPv6";
                            checked <=> root.prefer-ipv6;
                        }
                        RmxCheckBox {
                            text: "Fermer après";
                            checked <=> root.close-fw;
                        }
                        Rectangle { }
                    }
                }

                // Exclude + limit
                Rectangle {
                    height: 36px;
                    HorizontalLayout {
                        spacing: 10px;
                        RmxLabel { text: "Exclure :"; min-label-width: 70px; }
                        RmxTextInput {
                            text <=> root.exclude-text;
                            placeholder: ".git, node_modules, *.tmp";
                        }
                        RmxLabel { text: "Limite :"; min-label-width: 60px; }
                        RmxTextInput {
                            text <=> root.limit-text;
                            placeholder: "Mo/s";
                            min-width: 70px;
                            h-alignment: center;
                        }
                        Rectangle { }
                    }
                }

                // Pipeline
                Rectangle {
                    height: 36px;
                    HorizontalLayout {
                        spacing: 10px;
                        RmxLabel { text: "Pipeline :"; }
                        RmxSlider {
                            value <=> root.pipeline-float;
                            changed(value) => {
                                root.pipeline-text = "N=" + floor(value);
                                root.pipeline = round(value);
                            }
                        }
                        RmxValue { text: root.pipeline-text; min-width: 60px; }
                        Rectangle { }
                    }
                }

                // Progress bar
                Rectangle {
                    height: 40px;
                    VerticalLayout {
                        spacing: 4px;
                        ProgressIndicator {
                            progress: root.progress;
                            indeterminate: false;
                            height: 8px;
                        }
                        Text {
                            text: root.progress-text;
                            color: RmxTheme.crt;
                            font-family: "HelpMe";
                            font-size: 12px;
                            horizontal-alignment: center;
                            overflow: elide;
                        }
                    }
                }

                // QR + commande (mode réception)
                Rectangle {
                    height: 132px;
                    visible: root.mode == "receive" && root.qr-visible && !root.running;
                    HorizontalLayout {
                        spacing: 12px;
                        padding: 4px;
                        Rectangle {
                            width: 124px;
                            height: 124px;
                            background: RmxTheme.crt-voile;
                            Image {
                                x: 2px;
                                y: 2px;
                                width: 120px;
                                height: 120px;
                                source: root.qr-image;
                                image-fit: contain;
                            }
                        }
                        VerticalLayout {
                            spacing: 6px;
                            Text {
                                text: "Commande à lancer sur l'autre PC (QR : dtx://IP:port?code=...) :";
                                color: RmxTheme.crt;
                                font-family: "HelpMe";
                                font-size: 12px;
                                wrap: word-wrap;
                            }
                            cmd-field := TextInput {
                                read-only: true;
                                text: root.cmd-text;
                                color: RmxTheme.crt;
                                font-family: "HelpMe";
                                font-size: 12px;
                            }
                        }
                    }
                }

                // Approbation du manifeste (mode réception)
                Rectangle {
                    height: 96px;
                    visible: root.awaiting-approval;
                    background: RmxTheme.crt-voile;
                    VerticalLayout {
                        spacing: 6px;
                        padding: 8px;
                        Text {
                            text: root.manifest-text;
                            color: RmxTheme.crt;
                            font-family: "HelpMe";
                            font-size: 12px;
                            wrap: word-wrap;
                        }
                        HorizontalLayout {
                            spacing: 10px;
                            RmxButton {
                                text: "Accepter";
                                primary: true;
                                clicked => { root.accept-transfer(); }
                            }
                            RmxButton {
                                text: "Refuser";
                                accent: true;
                                clicked => { root.refuse-transfer(); }
                            }
                            Rectangle { }
                        }
                    }
                }

                // Action buttons
                Rectangle {
                    height: 44px;
                    HorizontalLayout {
                        spacing: 10px;
                        RmxButton {
                            text: "Démarrer";
                            primary: true;
                            enabled: !root.running && !root.awaiting-approval;
                            clicked => { root.start-transfer(); }
                        }
                        RmxButton {
                            text: "Annuler";
                            accent: true;
                            enabled: root.running;
                            clicked => { root.cancel-transfer(); }
                        }
                        RmxButton {
                            text: "Copier la commande";
                            visible: root.mode == "receive";
                            enabled: !root.running;
                            clicked => {
                                root.copy-command();
                                cmd-field.select-all();
                                cmd-field.copy();
                            }
                        }
                        RmxButton {
                            text: "Découvrir";
                            visible: root.mode == "send";
                            enabled: !root.running;
                            clicked => { root.discover(); }
                        }
                        Rectangle { }
                    }
                }

                // Log area
                Rectangle {
                    background: RmxTheme.crt-voile;
                    vertical-stretch: 1;
                    min-height: 140px;
                    log-scroll := ScrollView {
                        x: 5px;
                        y: 5px;
                        width: parent.width - 10px;
                        height: parent.height - 10px;
                        content-height: max(self.visible-height, log-input.preferred-height);
                        log-input := TextInput {
                            read-only: true;
                            single-line: false;
                            wrap: word-wrap;
                            text: root.log-text;
                            color: RmxTheme.crt;
                            font-family: "HelpMe";
                            font-size: 12px;
                        }
                    }
                }

                // Footer / hidden layer
                Rectangle {
                    height: 18px;
                    Text {
                        text: root.hidden;
                        color: RmxTheme.crt-voile;
                        font-family: "HelpMe";
                        font-size: 9px;
                        horizontal-alignment: center;
                    }
        // La couche cachée ne s'explique pas, elle se cherche.
                    Rectangle {
                        y: 16px;
                        height: 1px;
                        background: RmxTheme.crt-voile;
                    }
                }
        }
    }
}
