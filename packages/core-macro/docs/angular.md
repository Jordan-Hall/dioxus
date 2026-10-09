# Angular-style templates for Dioxus

The angular! procedural macro accepts one Rust string literal containing an Angular-style HTML template and lowers it through Dioxus's existing RSX parser and renderer.

## Example

    use dioxus::prelude::*;

    #[component]
    fn Counter() -> Element {
        let mut count = use_signal(|| 0);

        angular!(r#"
            <section class="counter">
                <h1>Angular-style templates in Dioxus</h1>
                <button
                    [disabled]="count() >= 10"
                    (click)="count += 1"
                >
                    Clicked {{ count() }} times
                </button>
                @if (count() >= 10) {
                    <p>Limit reached</p>
                } @else {
                    <p>Keep clicking</p>
                }
            </section>
        "#)
    }

## Supported template constructs

- HTML elements, void elements, static attributes, HTML comments, common named/numeric entities, and text interpolation using {{ rust_expression }}.
- Property bindings such as [disabled]="condition" and bind-disabled="condition".
- Event bindings such as (click)="handler($event)" and on-click="handler($event)". $event is translated to the generated Rust closure argument.
- Two-way binding syntax such as [(value)]="signal" and bindon-value="signal". The current adapter uses Dioxus form events for value and checked.
- @if / @else if / @else (including Option aliases using @if (option; as value)), @for (item of items; track item.id) / @empty, @switch / @case / @default, and @let name = rust_expression;.
- @boundary with an optional @error block, lowered to Dioxus's ErrorBoundary.
- @defer with @placeholder, @loading, and @error blocks, lowered to Dioxus suspense/error boundaries.

## Expression model and semantic differences

Expressions inside interpolation and bindings are Rust expressions, not TypeScript or Angular's JavaScript-like template expression language. Rust type checking and ownership rules apply at the normal macro expansion site.

This macro is a syntax frontend, not a port of Angular's complete compiler or runtime. In particular:

- @defer currently selects a Dioxus suspense fallback but does not implement Angular's idle/viewport/interaction/timer trigger scheduling or automatic JavaScript chunk splitting. Trigger clauses are parsed for forward compatibility but do not change runtime behavior.
- Angular @if (...; as alias) is mapped to Rust Option matching: the condition must evaluate to Option<T>.
- #templateRef / ref-name declarations do not have Angular's template variable semantics; declare a Dioxus NodeRef in Rust and use the same identifier in the template.
- Angular directives, pipes, dependency injection, JavaScript truthiness, signal-specific TypeScript unwrapping, and Angular input/output metadata are not synthesized. Components must be ordinary Dioxus components with Rust props/events.
- Angular's $index, $first, $last, $even, $odd, and $count loop locals are not injected. Use Rust iterator expressions and explicit item bindings.

Unsupported or invalid template syntax produces a compile error at the macro call site. The resulting Dioxus nodes go through the established RSX code-generation path instead of creating a parallel renderer.
