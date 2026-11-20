//! Settings' Account card in the glass look, as the mockup's: who plays now (a big head, the
//! name, how), and the Microsoft card beside it with its white Sign in with Microsoft button.
//!
//! Not in the mockup, kept in its patterns: the Microsoft accounts already signed in (in the
//! Microsoft card, with Play as, Refresh and Sign out), the sign-in's code, progress and problems.

use super::*;
use crate::glass_ui::{self as ui, white, Tone};
use crate::servers_page::Field;
use horizon_theme::glass::{fill, ink, radius};

impl Launcher {
    pub(super) fn glass_account(&mut self, cx: &mut Context<Self>) -> Div {
        let wide = self.window_width >= crate::glass_home::WIDE;
        let left = self.glass_who_plays(cx);
        let right = self.glass_microsoft_card(cx);
        if wide {
            div()
                .flex()
                .flex_row()
                .gap(px(8.))
                .child(left.flex_1().min_w(px(280.)).p(px(20.)))
                .child(right.flex_none().w(px(360.)))
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(left.w_full().min_w_0().p(px(20.)))
                .child(right.w_full())
        }
    }

    pub(super) fn add_offline_player(&mut self) {
        let name = self.servers.value(Field::OfflinePlayerName).trim().to_string();
        if !account::valid_offline_name(&name) {
            self.servers.invalid = Some((
                Field::OfflinePlayerName,
                "A Minecraft name has 3 to 16 letters, digits or underscores.",
            ));
            return;
        }
        self.servers.invalid = None;
        self.servers.text(Field::OfflinePlayerName).set(String::new());
        if let Ok(()) = self.accounts.play_as_offline(&name) {
            let _ = self.accounts.save();
            self.load_profile_heads();
        }
    }

    pub(super) fn save_msa_client_id(&mut self) {
        let raw = self.servers.value(Field::MsaClientId).trim().to_string();
        self.settings.msa_client_id = if raw.is_empty() { None } else { Some(raw) };
        let _ = self.settings.save();
    }

    /// The player now, and saved offline profiles.
    fn glass_who_plays(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let (uuid, name, status) = match self.accounts.selected() {
            Some(a) => (
                Some(a.uuid.clone()),
                a.name.clone(),
                (Tone::Live, tr("Microsoft account · online servers too")),
            ),
            None => {
                let name = self.accounts.active_name();
                (
                    None,
                    name,
                    (Tone::Live, tr("Offline account · singleplayer & offline servers")),
                )
            }
        };
        let head = div()
            .flex_none()
            .size(px(56.))
            .rounded(px(14.))
            .overflow_hidden()
            .child(
                self.profile_heads
                    .draw(uuid.as_deref(), 56., self.scale, &t),
            );
        let header = div().flex().items_center().gap(px(16.)).child(head).child(
            div()
                .flex()
                .flex_col()
                .min_w_0()
                .gap(px(10.))
                .child(
                    div()
                        .font_family(horizon_theme::glass::DISPLAY_FONT)
                        .font_weight(FontWeight::SEMIBOLD)
                        // A compact summary: the top bar and the account rows name the
                        // player too, so this is no page headline.
                        .text_size(px(22.))
                        .line_height(px(28.))
                        .text_color(white(ink::FULL))
                        .truncate()
                        .child(name),
                )
                .child(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(8.))
                        .child(ui::dot(status.0).mt(px(7.)))
                        .child(
                            ui::text(14., FontWeight::NORMAL, 0.65)
                                .flex_1()
                                .min_w_0()
                                .line_height(px(21.))
                                .child(status.1),
                        ),
                ),
        );

        let now_offline = self.accounts.selected().is_none();
        let active_offline_name = self
            .accounts
            .offline_selected
            .clone()
            .unwrap_or_else(|| self.accounts.active_name());

        let mut offline_list = div().flex().flex_col().gap(px(6.));
        let offline_names = self.accounts.offline.clone();
        for (i, off_name) in offline_names.iter().enumerate() {
            let is_active = now_offline && off_name.eq_ignore_ascii_case(&active_offline_name);
            let name_for_switch = off_name.clone();
            let name_for_del = off_name.clone();
            let id = SharedString::from(format!("offline-prof-{i}"));

            let row = div()
                .id(id)
                .flex()
                .items_center()
                .justify_between()
                .h(px(40.))
                .px(px(12.))
                .rounded(px(radius::ROW))
                .bg(white(if is_active { 0.08 } else { 0.03 }))
                .border_1()
                .border_color(white(if is_active { ui::edge::STRONG } else { ui::edge::REST }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(
                            div()
                                .size(px(24.))
                                .rounded(px(6.))
                                .overflow_hidden()
                                .child(self.profile_heads.draw(Some(off_name.as_str()), 24., self.scale, &t)),
                        )
                        .child(
                            ui::text(
                                14.,
                                if is_active { FontWeight::SEMIBOLD } else { FontWeight::NORMAL },
                                ink::FULL,
                            )
                            .child(off_name.clone()),
                        )
                        .when(is_active, |d| {
                            d.child(
                                div()
                                    .px(px(6.))
                                    .py(px(2.))
                                    .rounded(px(4.))
                                    .bg(white(0.12))
                                    .child(
                                        ui::text(11., FontWeight::MEDIUM, ink::FULL)
                                            .child(tr("Active")),
                                    ),
                            )
                        }),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .when(!is_active, |d| {
                            let n = name_for_switch.clone();
                            d.child(
                                keyboard(
                                    div()
                                        .id(SharedString::from(format!("switch-off-{i}")))
                                        .px(px(10.))
                                        .py(px(4.))
                                        .rounded(px(radius::CONTROL))
                                        .bg(white(fill::BUTTON))
                                        .hover(|s| s.bg(white(fill::BUTTON_HOVER)))
                                        .cursor_pointer()
                                        .child(
                                            ui::text(12., FontWeight::MEDIUM, ink::FULL)
                                                .child(tr("Switch")),
                                        )
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.play_as(&n);
                                            cx.notify();
                                        })),
                                    &t,
                                ),
                            )
                        })
                        .child(
                            keyboard(
                                div()
                                    .id(SharedString::from(format!("del-off-{i}")))
                                    .p(px(6.))
                                    .rounded(px(radius::CONTROL))
                                    .hover(|s| s.bg(white(0.1)))
                                    .cursor_pointer()
                                    .child(ui::icon("delete-02", 14., white(ink::DIM)))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        let _ = this.accounts.remove_offline(&name_for_del);
                                        let _ = this.accounts.save();
                                        this.load_profile_heads();
                                        cx.notify();
                                    })),
                                &t,
                            ),
                        ),
                );
            offline_list = offline_list.child(row);
        }

        let add_field = self.field(
            Field::OfflinePlayerName,
            "New player name",
            180.,
            cx,
        );
        let add_btn = keyboard(
            div()
                .id("add-offline-btn")
                .flex()
                .items_center()
                .gap(px(6.))
                .h(px(36.))
                .px(px(12.))
                .rounded(px(radius::CONTROL))
                .bg(white(fill::BUTTON))
                .hover(|s| s.bg(white(fill::BUTTON_HOVER)))
                .cursor_pointer()
                .child(ui::icon("plus-sign", 16., white(ink::FULL)))
                .child(ui::text(13., FontWeight::MEDIUM, ink::FULL).child(tr("Add & Switch"))),
            &t,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.add_offline_player();
            cx.notify();
        }));

        let add_row = div()
            .mt(px(8.))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(add_field)
            .child(add_btn);

        let offline_section = div()
            .mt(px(28.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .font_family(horizon_theme::glass::FONT)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(px(14.))
                    .text_color(white(ink::FULL))
                    .child(tr("Offline profiles")),
            )
            .when(offline_names.is_empty(), |d| {
                d.child(
                    ui::text(13., FontWeight::NORMAL, 0.5)
                        .child(tr("No offline profiles saved yet. Add one below:")),
                )
            })
            .when(!offline_names.is_empty(), |d| d.child(offline_list))
            .child(add_row);

        div()
            .flex()
            .flex_col()
            .child(header)
            .child(offline_section)
    }

    /// The Microsoft card: the accounts signed in here, then adding one (the mockup's card).
    fn glass_microsoft_card(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let has_accounts = !self.accounts.accounts.is_empty();
        let (rows, _) = if has_accounts {
            ui::flat_rows(|| self.microsoft_rows(cx))
        } else {
            (div(), 0)
        };
        let (title, detail, action) = self.sign_in_parts(cx);
        let configured = account::client_id().is_some();
        let idle = self.sign_in_state.is_none()
            || matches!(self.sign_in_state, Some(SignInState::Done(_)));
        let first = !has_accounts && self.sign_in_target.is_none();
        let heading = if first {
            tr("Play online with your Microsoft account").to_string()
        } else {
            title
        };
        let note: AnyElement = if first && configured && self.sign_in_state.is_none() {
            div()
                .child(tr(
                    "Sign in with the account that owns Minecraft: Java Edition. Your name and \
                     skin come along to online servers, like in other launchers.",
                ))
                .into_any_element()
        } else {
            detail.into_any_element()
        };

        if !self.servers.has_focus(Field::MsaClientId) {
            if let Some(id) = &self.settings.msa_client_id {
                if self.servers.value(Field::MsaClientId).is_empty() {
                    self.servers.text(Field::MsaClientId).set(id.clone());
                }
            }
        }

        let client_id_config: Option<AnyElement> = if !configured && idle {
            let field = self.field(
                Field::MsaClientId,
                "Azure Client ID (GUID)",
                220.,
                cx,
            );
            let save_btn = keyboard(
                div()
                    .id("save-msa-client-id-btn")
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .h(px(36.))
                    .px(px(14.))
                    .rounded(px(radius::CONTROL))
                    .bg(white(fill::BUTTON))
                    .hover(|s| s.bg(white(fill::BUTTON_HOVER)))
                    .cursor_pointer()
                    .child(ui::text(13., FontWeight::MEDIUM, ink::FULL).child(tr("Save"))),
                &t,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.save_msa_client_id();
                cx.notify();
            }));

            Some(
                div()
                    .mt(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        ui::text(12., FontWeight::NORMAL, 0.75).child(tr(
                            "Enter a Microsoft Azure Application (Client) ID to enable sign-in:",
                        )),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(field)
                            .child(save_btn),
                    )
                    .into_any_element(),
            )
        } else if configured && self.settings.msa_client_id.is_some() && idle {
            Some(
                div()
                    .mt(px(12.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        ui::text(12., FontWeight::NORMAL, 0.5)
                            .child(tr("Using custom Azure Client ID")),
                    )
                    .child(
                        keyboard(
                            div()
                                .id("clear-msa-client-id-btn")
                                .px(px(8.))
                                .py(px(4.))
                                .rounded(px(radius::CONTROL))
                                .hover(|s| s.bg(white(0.08)))
                                .cursor_pointer()
                                .child(
                                    ui::text(12., FontWeight::NORMAL, ink::FAINT)
                                        .child(tr("Reset")),
                                ),
                            &t,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.settings.msa_client_id = None;
                            let _ = this.settings.save();
                            this.servers.text(Field::MsaClientId).set(String::new());
                            cx.notify();
                        })),
                    )
                    .into_any_element(),
            )
        } else {
            None
        };

        // Adding an account is the mockup's white button; signing one in again, the code and
        // the waiting keep their own buttons.
        let button: Option<AnyElement> = if configured && idle && self.sign_in_target.is_none() {
            Some(
                keyboard(
                    div()
                        .id("glass-sign-in")
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap(px(10.))
                        .h(px(48.))
                        .w_full()
                        .mt(px(24.))
                        .rounded(px(radius::ROW))
                        .bg(gpui::white())
                        // On a light plate, Microsoft's white button keeps an edge.
                        .when(ui::light(), |d| d.border_1().border_color(ui::white(0.15)))
                        .cursor_pointer()
                        .hover(|s| s.opacity(0.9))
                        .font_family(horizon_theme::glass::FONT)
                        .text_size(px(15.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(gpui::rgb(0x100a1f))
                        .child(microsoft::logo(18.))
                        .child(tr("Sign in with Microsoft")),
                    &t,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.start_sign_in();
                    cx.notify();
                }))
                .into_any_element(),
            )
        } else {
            action.map(|a| div().mt(px(20.)).child(a).into_any_element())
        };
        div()
            .flex()
            .flex_col()
            .rounded(px(16.))
            .bg(white(0.045))
            .border_1()
            .border_color(white(ui::edge::REST))
            .p(px(24.))
            .when(has_accounts, |d| {
                d.child(rows.mx(px(-16.)).mt(px(-16.)).mb(px(16.)))
            })
            .child(microsoft::logo(36.))
            .child(
                div()
                    .mt(px(20.))
                    .font_family(horizon_theme::glass::DISPLAY_FONT)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(px(22.))
                    .line_height(px(27.5))
                    .text_color(white(ink::FULL))
                    .child(heading),
            )
            .child(
                ui::text(14., FontWeight::NORMAL, 0.65)
                    .mt(px(8.))
                    .line_height(px(21.7))
                    .child(note),
            )
            .children(button)
            .children(client_id_config)
            .when(configured, |d| {
                d.child(
                    ui::text(12., FontWeight::NORMAL, 0.45)
                        .mt(px(12.))
                        .line_height(px(18.))
                        .child(tr(
                            "Microsoft's own page opens in your browser. Horizon never sees your \
                             password.",
                        )),
                )
            })
    }
}
