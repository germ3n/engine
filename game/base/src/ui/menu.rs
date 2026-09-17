use crate::ui::window::Window;
use crate::ui::Color;
use crate::GameState;

pub fn draw_menu<T: Window>(window: &mut T, game: &mut GameState) {
    game.script_engine.run_hook("MenuPaint", ()); // render the menu

    window.render_text();
}