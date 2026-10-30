//! Profile model preferences; the existing Theorem authority resolves credentials
//! and validates every requested model against its configured catalog.
use gpui_kit::component::{
    Selectable as _, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex,
    input::{Input, InputState},
    v_flex,
};
use gpui_kit::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Window,
    div, px,
};
use theorem_surface_contracts::model_routing::{
    DecisionCredential, ModelReference, ModelRole, ModelRoutingRequest,
};

pub(super) fn routing_label(cx: &App) -> String {
    let routing = super::saved::routing(cx);
    format!(
        "{} · {}",
        routing.role.label(),
        routing
            .assignments
            .requested(routing.role)
            .map(ModelReference::name)
            .unwrap_or_else(|| "Automatic".into())
    )
}

pub(crate) fn open(window: &mut Window, cx: &mut App) {
    let panel = cx.new(|cx| Models::new(window, cx));
    window.open_sheet(cx, move |sheet, _, _| {
        sheet
            .title("Models and workflow")
            .size(px(520.))
            .child(panel.clone())
    });
}

struct Models {
    routing: ModelRoutingRequest,
    fields: Vec<(ModelRole, Entity<InputState>)>,
    fallback: Entity<InputState>,
    error: String,
}
impl Models {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let routing = super::saved::routing(cx);
        let fields = ModelRole::ALL
            .into_iter()
            .map(|role| {
                let value = routing
                    .assignments
                    .overrides
                    .get(&role)
                    .map(ModelReference::name)
                    .unwrap_or_default();
                (
                    role,
                    cx.new(|cx| {
                        InputState::new(window, cx)
                            .default_value(value)
                            .placeholder(if role == ModelRole::Decisions {
                                "typesafe/jev-1.13.0"
                            } else if role == ModelRole::Main {
                                "Automatic"
                            } else {
                                "Inherit main assistant"
                            })
                    }),
                )
            })
            .collect();
        let fallback = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(
                    routing
                        .assignments
                        .decision_fallback
                        .as_ref()
                        .map(ModelReference::name)
                        .unwrap_or_default(),
                )
                .placeholder("No typed-decision fallback")
        });
        Self {
            routing,
            fields,
            fallback,
            error: String::new(),
        }
    }
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut routing = self.routing.clone();
        routing.assignments.overrides.clear();
        for (role, input) in &self.fields {
            let value = input.read(cx).value().to_string();
            if value.trim().is_empty() {
                continue;
            }
            match ModelReference::parse(&value) {
                Ok(model) => {
                    routing.assignments.overrides.insert(*role, model);
                }
                Err(error) => {
                    self.error = format!("{}: {error}", role.label());
                    cx.notify();
                    return;
                }
            }
        }
        let fallback = self.fallback.read(cx).value().to_string();
        routing.assignments.decision_fallback = if fallback.trim().is_empty() {
            None
        } else {
            match ModelReference::parse(&fallback) {
                Ok(model) => Some(model),
                Err(error) => {
                    self.error = format!("Typed-decision fallback: {error}");
                    cx.notify();
                    return;
                }
            }
        };
        super::saved::set_routing(routing, cx);
        window.close_sheet(cx);
        window.refresh();
    }
}
impl Render for Models {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex().id("model-preferences").max_h(window.viewport_size().height - px(110.)).overflow_y_scroll().p_4().gap_3()
            .child("Workflow for the next message")
            .child(h_flex().flex_wrap().gap_1().children(ModelRole::ALL.into_iter().filter(|role| *role != ModelRole::Decisions).map(|role| {
                Button::new(SharedString::from(format!("workflow-{role:?}"))).small().ghost().label(role.label()).selected(self.routing.role == role)
                    .on_click(cx.listener(move |this, _, _, cx| { this.routing.role = role; cx.notify(); }))
            })))
            .child("Model assignments")
            .child(div().text_sm().child("Enter provider/model-id from your Theorem catalog. Blank roles inherit Main; blank Main uses Automatic. Decisions use their own provider."))
            .children(self.fields.iter().map(|(role, field)| v_flex().gap_1().child(div().text_sm().child(role.label())).child(Input::new(field).aria_label(role.label()))))
            .child(v_flex().gap_1().child(div().text_sm().child("Typed-decision fallback")).child(Input::new(&self.fallback).aria_label("Typed-decision fallback")))
            .child(div().text_sm().child("Jev credential on your Theorem authority"))
            .child(h_flex().flex_wrap().gap_1().children([
                (DecisionCredential::AuthorityDefault, "Authority default"),
                (DecisionCredential::Keychain, "Keychain"),
                (DecisionCredential::Environment, "Environment"),
            ].into_iter().map(|(policy, label)| Button::new(SharedString::from(format!("decision-credential-{policy:?}"))).small().ghost()
                .label(label).selected(self.routing.assignments.decision_credential == policy)
                .on_click(cx.listener(move |this, _, _, cx| { this.routing.assignments.decision_credential = policy; cx.notify(); })))))
            .child(Checkbox::new("allow-model-fallback").label("Allow Automatic if an assigned chat model cannot be bound")
                .checked(self.routing.assignments.allow_fallback).on_change(cx.listener(|this, value, _, cx| { this.routing.assignments.allow_fallback = *value; cx.notify(); })))
            .child(div().text_sm().child("Credentials are resolved by your Theorem authority. No keys or endpoints are stored in these preferences."))
            .child(div().text_sm().child(self.error.clone()))
            .child(Button::new("save-models").primary().label("Save").on_click(cx.listener(Self::save_clicked)))
    }
}
impl Models {
    fn save_clicked(
        &mut self,
        _: &gpui_kit::ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.save(window, cx);
    }
}
