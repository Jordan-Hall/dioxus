use dioxus::prelude::*;

#[allow(dead_code)]
fn angular_template_compiles() -> Element {
    let mut count = use_signal(|| 0);
    angular!(
        r#"
        <section class="counter">
            <h1>Counter</h1>
            <button [disabled]="count() >= 2" (click)="count += 1">
                Count: {{ count() }}
            </button>
            @if (count() >= 2) {
                <p>Done</p>
            } @else {
                <p>Keep going</p>
            }
        </section>
        "#
    )
}

#[allow(dead_code)]
fn angular_loop_compiles() -> Element {
    angular!(
        r#"
        @for (item of [1, 2, 3]; track item; let index = $index, total = $count) {
            <span>{{ index }} / {{ total }}: {{ item }}</span>
        } @empty {
            <p>Empty</p>
        }
        "#
    )
}

#[allow(dead_code)]
fn angular_defer_compiles() -> Element {
    angular!(
        r#"
        @defer (on viewport) {
            <section>Deferred content</section>
        } @placeholder (minimum 500ms) {
            <p>Loading</p>
        } @error {
            <p>Failed to load</p>
        }
        "#
    )
}

#[allow(dead_code)]
fn angular_boundary_compiles() -> Element {
    angular!(
        r#"
        @boundary {
            <section>Protected content</section>
        } @error {
            <p>Something failed</p>
        }
        "#
    )
}
