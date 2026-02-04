use crate::termwindow::box_model::*;
use crate::termwindow::modal::Modal;
use crate::termwindow::render::corners::{
    BOTTOM_LEFT_ROUNDED_CORNER, BOTTOM_RIGHT_ROUNDED_CORNER, TOP_LEFT_ROUNDED_CORNER,
    TOP_RIGHT_ROUNDED_CORNER,
};
use crate::termwindow::TermWindow;
use crate::utilsprites::RenderMetrics;
use config::keyassignment::{
    KeyAssignment, PaneSelectArguments, PaneSelectMode, RotationDirection, SpawnCommand,
    SpawnTabDomain,
};
use config::{Dimension, DimensionContext};
use std::cell::{Ref, RefCell};
use termwiz::nerdfonts::NERD_FONTS;
use wezterm_term::{KeyCode, KeyModifiers, MouseEvent};
use window::color::LinearRgba;

/// A menu item in the context menu
enum MenuItem {
    Entry {
        label: &'static str,
        icon: Option<&'static str>,
        action: KeyAssignment,
    },
    Separator,
}

pub struct ContextMenu {
    element: RefCell<Option<Vec<ComputedElement>>>,
    /// Currently selected/hovered row (-1 = none)
    selected_row: RefCell<i32>,
    items: Vec<MenuItem>,
    /// Actual rendered position of menu (in pixels), set after first render
    menu_x: RefCell<f32>,
    menu_y: RefCell<f32>,
    /// Initial mouse position (used for computing menu position)
    initial_mouse_x: f32,
    initial_mouse_y: f32,
    /// Row height in pixels (set after first render)
    row_height: RefCell<f32>,
    /// Menu dimensions (set after first render)
    menu_width: RefCell<f32>,
    menu_height: RefCell<f32>,
}

impl ContextMenu {
    pub fn new(term_window: &mut TermWindow, mouse_x: isize, mouse_y: isize) -> Self {
        let mut items = vec![
            // Split pane options
            MenuItem::Entry {
                label: "Split Pane Right",
                icon: Some("cod_split_horizontal"),
                action: KeyAssignment::SplitHorizontal(SpawnCommand {
                    domain: SpawnTabDomain::CurrentPaneDomain,
                    ..Default::default()
                }),
            },
            MenuItem::Entry {
                label: "Split Pane Down",
                icon: Some("cod_split_vertical"),
                action: KeyAssignment::SplitVertical(SpawnCommand {
                    domain: SpawnTabDomain::CurrentPaneDomain,
                    ..Default::default()
                }),
            },
        ];

        // Add pane manipulation options if there are multiple panes
        if let Some(tab) = mux::Mux::get().get_active_tab_for_window(term_window.mux_window_id) {
            if tab.count_panes().unwrap_or(1) > 1 {
                items.push(MenuItem::Separator);
                items.push(MenuItem::Entry {
                    label: "Swap Pane Up",
                    icon: Some("cod_arrow_up"),
                    action: KeyAssignment::RotatePanes(RotationDirection::CounterClockwise),
                });
                items.push(MenuItem::Entry {
                    label: "Swap Pane Down",
                    icon: Some("cod_arrow_down"),
                    action: KeyAssignment::RotatePanes(RotationDirection::Clockwise),
                });
                items.push(MenuItem::Entry {
                    label: "Select Pane to Swap",
                    icon: Some("cod_replace"),
                    action: KeyAssignment::PaneSelect(PaneSelectArguments {
                        mode: PaneSelectMode::SwapWithActiveKeepFocus,
                        ..Default::default()
                    }),
                });
            }
        }

        // Zoom option (only if multiple panes)
        if let Some(tab) = mux::Mux::get().get_active_tab_for_window(term_window.mux_window_id) {
            if tab.count_panes().unwrap_or(1) > 1 {
                items.push(MenuItem::Separator);
                items.push(MenuItem::Entry {
                    label: "Toggle Zoom",
                    icon: Some("cod_screen_full"),
                    action: KeyAssignment::TogglePaneZoomState,
                });
            }
        }

        // New tab/window options
        items.push(MenuItem::Separator);
        items.push(MenuItem::Entry {
            label: "New Tab",
            icon: Some("cod_add"),
            action: KeyAssignment::SpawnTab(SpawnTabDomain::CurrentPaneDomain),
        });
        items.push(MenuItem::Entry {
            label: "New Window",
            icon: Some("cod_window"),
            action: KeyAssignment::SpawnWindow,
        });

        // Tab reordering options
        items.push(MenuItem::Separator);
        items.push(MenuItem::Entry {
            label: "Move Tab Left",
            icon: Some("cod_arrow_left"),
            action: KeyAssignment::MoveTabRelative(-1),
        });
        items.push(MenuItem::Entry {
            label: "Move Tab Right",
            icon: Some("cod_arrow_right"),
            action: KeyAssignment::MoveTabRelative(1),
        });

        // Close pane option if there are multiple panes
        if let Some(tab) = mux::Mux::get().get_active_tab_for_window(term_window.mux_window_id) {
            if tab.count_panes().unwrap_or(1) > 1 {
                items.push(MenuItem::Separator);
                items.push(MenuItem::Entry {
                    label: "Close Pane",
                    icon: Some("cod_close"),
                    action: KeyAssignment::CloseCurrentPane { confirm: false },
                });
            }
        }

        Self {
            element: RefCell::new(None),
            selected_row: RefCell::new(0), // Start with first item selected
            items,
            menu_x: RefCell::new(0.0),
            menu_y: RefCell::new(0.0),
            initial_mouse_x: mouse_x as f32,
            initial_mouse_y: mouse_y as f32,
            row_height: RefCell::new(0.0),
            menu_width: RefCell::new(0.0),
            menu_height: RefCell::new(0.0),
        }
    }

    fn compute(
        term_window: &mut TermWindow,
        items: &[MenuItem],
        selected_row: i32,
        initial_mouse_x: f32,
        initial_mouse_y: f32,
    ) -> anyhow::Result<(Vec<ComputedElement>, f32, f32, f32, f32, f32)> {
        let font = term_window
            .fonts
            .command_palette_font()
            .expect("to resolve command palette font");
        let metrics = RenderMetrics::with_font_metrics(&font.metrics());
        let row_height = metrics.cell_size.height as f32;

        let solid_bg_color: InheritableColor = term_window
            .config
            .command_palette_bg_color
            .to_linear()
            .into();
        let solid_fg_color: InheritableColor = term_window
            .config
            .command_palette_fg_color
            .to_linear()
            .into();

        let mut elements: Vec<Element> = vec![];

        for (idx, item) in items.iter().enumerate() {
            match item {
                MenuItem::Entry { label, icon, .. } => {
                    let icon_char = match icon {
                        Some(nf) => NERD_FONTS.get(*nf).unwrap_or(&' '),
                        None => &' ',
                    };

                    let (bg, text) = if idx as i32 == selected_row {
                        (solid_fg_color.clone(), solid_bg_color.clone())
                    } else {
                        (LinearRgba::TRANSPARENT.into(), solid_fg_color.clone())
                    };

                    let row = vec![
                        Element::new(&font, ElementContent::Text(icon_char.to_string()))
                            .min_width(Some(Dimension::Cells(2.))),
                        Element::new(&font, ElementContent::Text(label.to_string())),
                    ];

                    elements.push(
                        Element::new(&font, ElementContent::Children(row))
                            .colors(ElementColors {
                                border: BorderColor::default(),
                                bg,
                                text,
                            })
                            .padding(BoxDimension {
                                left: Dimension::Cells(0.5),
                                right: Dimension::Cells(0.5),
                                top: Dimension::Cells(0.1),
                                bottom: Dimension::Cells(0.1),
                            })
                            .min_width(Some(Dimension::Cells(20.)))
                            .display(DisplayType::Block),
                    );
                }
                MenuItem::Separator => {
                    // Render a horizontal line for separator
                    elements.push(
                        Element::new(&font, ElementContent::Text("─".repeat(20)))
                            .colors(ElementColors {
                                border: BorderColor::default(),
                                bg: LinearRgba::TRANSPARENT.into(),
                                text: solid_fg_color.clone(),
                            })
                            .padding(BoxDimension {
                                left: Dimension::Cells(0.5),
                                right: Dimension::Cells(0.5),
                                top: Dimension::Cells(0.1),
                                bottom: Dimension::Cells(0.1),
                            })
                            .min_width(Some(Dimension::Cells(20.)))
                            .display(DisplayType::Block),
                    );
                }
            }
        }

        let dimensions = term_window.dimensions;

        let element = Element::new(&font, ElementContent::Children(elements))
            .colors(ElementColors {
                border: BorderColor::new(
                    term_window
                        .config
                        .command_palette_bg_color
                        .to_linear()
                        .into(),
                ),
                bg: term_window
                    .config
                    .command_palette_bg_color
                    .to_linear()
                    .into(),
                text: term_window
                    .config
                    .command_palette_fg_color
                    .to_linear()
                    .into(),
            })
            .margin(BoxDimension {
                left: Dimension::Cells(0.25),
                right: Dimension::Cells(0.25),
                top: Dimension::Cells(0.25),
                bottom: Dimension::Cells(0.25),
            })
            .padding(BoxDimension {
                left: Dimension::Cells(0.25),
                right: Dimension::Cells(0.25),
                top: Dimension::Cells(0.25),
                bottom: Dimension::Cells(0.25),
            })
            .border(BoxDimension::new(Dimension::Pixels(1.)))
            .border_corners(Some(Corners {
                top_left: SizedPoly {
                    width: Dimension::Cells(0.25),
                    height: Dimension::Cells(0.25),
                    poly: TOP_LEFT_ROUNDED_CORNER,
                },
                top_right: SizedPoly {
                    width: Dimension::Cells(0.25),
                    height: Dimension::Cells(0.25),
                    poly: TOP_RIGHT_ROUNDED_CORNER,
                },
                bottom_left: SizedPoly {
                    width: Dimension::Cells(0.25),
                    height: Dimension::Cells(0.25),
                    poly: BOTTOM_LEFT_ROUNDED_CORNER,
                },
                bottom_right: SizedPoly {
                    width: Dimension::Cells(0.25),
                    height: Dimension::Cells(0.25),
                    poly: BOTTOM_RIGHT_ROUNDED_CORNER,
                },
            }));

        // Calculate menu dimensions
        // Account for: items, per-item padding (0.2 cells each), outer margin/padding/border (~1.5 cells)
        let menu_width = 25. * metrics.cell_size.width as f32;
        let menu_height = (items.len() as f32 * 1.2 + 1.5) * row_height;

        // Position the menu at the mouse location, but ensure it stays within the window
        let menu_x = initial_mouse_x
            .min(dimensions.pixel_width as f32 - menu_width)
            .max(0.);
        let menu_y = initial_mouse_y
            .min(dimensions.pixel_height as f32 - menu_height)
            .max(0.);

        let computed = term_window.compute_element(
            &LayoutContext {
                height: DimensionContext {
                    dpi: dimensions.dpi as f32,
                    pixel_max: dimensions.pixel_height as f32,
                    pixel_cell: metrics.cell_size.height as f32,
                },
                width: DimensionContext {
                    dpi: dimensions.dpi as f32,
                    pixel_max: dimensions.pixel_width as f32,
                    pixel_cell: metrics.cell_size.width as f32,
                },
                bounds: euclid::rect(menu_x, menu_y, menu_width, menu_height),
                metrics: &metrics,
                gl_state: term_window.render_state.as_ref().unwrap(),
                zindex: 100,
            },
            &element,
        )?;

        Ok((
            vec![computed],
            row_height,
            menu_x,
            menu_y,
            menu_width,
            menu_height,
        ))
    }

    /// Check if a given row index is a selectable entry (not a separator)
    fn is_selectable(&self, row: i32) -> bool {
        if row < 0 || row >= self.items.len() as i32 {
            return false;
        }
        matches!(self.items[row as usize], MenuItem::Entry { .. })
    }

    fn move_up(&self) {
        let mut row = self.selected_row.borrow_mut();
        let mut new_row = *row - 1;
        // Skip over separators
        while new_row >= 0 && !self.is_selectable(new_row) {
            new_row -= 1;
        }
        if new_row >= 0 {
            *row = new_row;
        }
        // Clear cached element to force re-render with new selection
        self.element.borrow_mut().take();
    }

    fn move_down(&self) {
        let limit = self.items.len() as i32;
        let mut row = self.selected_row.borrow_mut();
        let mut new_row = *row + 1;
        // Skip over separators
        while new_row < limit && !self.is_selectable(new_row) {
            new_row += 1;
        }
        if new_row < limit {
            *row = new_row;
        }
        // Clear cached element to force re-render with new selection
        self.element.borrow_mut().take();
    }

    fn set_selection(&self, row: i32) {
        // Don't select separators
        if !self.is_selectable(row) {
            return;
        }
        let mut selected = self.selected_row.borrow_mut();
        if *selected != row {
            *selected = row;
            // Clear cached element to force re-render with new selection
            self.element.borrow_mut().take();
        }
    }

    fn activate_selected(&self, term_window: &mut TermWindow) {
        let selected_idx = *self.selected_row.borrow();
        if selected_idx >= 0 {
            if let Some(MenuItem::Entry { action, .. }) = self.items.get(selected_idx as usize) {
                let action = action.clone();
                term_window.cancel_modal();

                if let Some(pane) = term_window.get_active_pane_or_overlay() {
                    if let Err(err) = term_window.perform_key_assignment(&pane, &action) {
                        log::error!("Error performing context menu action: {err:#}");
                    }
                }
            }
        }
    }

    /// Calculate which menu row is at the given pixel coordinates
    /// Returns -1 if outside the menu
    fn row_at_coords(&self, x: f32, y: f32) -> i32 {
        let menu_x = *self.menu_x.borrow();
        let menu_y = *self.menu_y.borrow();
        let menu_width = *self.menu_width.borrow();
        let menu_height = *self.menu_height.borrow();
        let row_height = *self.row_height.borrow();

        if row_height <= 0.0 {
            return -1;
        }

        // Check if coordinates are within menu bounds
        if x < menu_x || x > menu_x + menu_width || y < menu_y || y > menu_y + menu_height {
            return -1;
        }

        // Calculate row:
        // - Outer margin/padding: ~0.5 cells
        // - Each item height: ~1.2 cells (text + 0.2 cells padding)
        let padding_top = row_height * 0.75;
        let item_height = row_height * 1.2;
        let relative_y = y - menu_y - padding_top;

        if relative_y < 0.0 {
            return 0; // Click in top padding area -> first item
        }

        let row = (relative_y / item_height) as i32;
        if row >= 0 && row < self.items.len() as i32 {
            row
        } else {
            -1
        }
    }
}

impl Modal for ContextMenu {
    fn perform_assignment(
        &self,
        _assignment: &KeyAssignment,
        _term_window: &mut TermWindow,
    ) -> bool {
        false
    }

    fn mouse_event(&self, event: MouseEvent, term_window: &mut TermWindow) -> anyhow::Result<()> {
        // Get actual pixel coordinates from the stored window event
        let (mouse_x, mouse_y) = term_window
            .current_mouse_event
            .as_ref()
            .map(|e| (e.coords.x as f32, e.coords.y as f32))
            .unwrap_or((0.0, 0.0));

        let row = self.row_at_coords(mouse_x, mouse_y);

        match event.kind {
            wezterm_term::input::MouseEventKind::Move => {
                // Update selection on hover
                if row >= 0 {
                    self.set_selection(row);
                }
            }
            wezterm_term::input::MouseEventKind::Press => {
                if row >= 0 {
                    self.set_selection(row);
                    self.activate_selected(term_window);
                } else {
                    // Click outside menu - close it
                    term_window.cancel_modal();
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn key_down(
        &self,
        key: KeyCode,
        mods: KeyModifiers,
        term_window: &mut TermWindow,
    ) -> anyhow::Result<bool> {
        match (key, mods) {
            (KeyCode::Escape, KeyModifiers::NONE)
            | (KeyCode::Char('q'), KeyModifiers::NONE)
            | (KeyCode::Char('c'), KeyModifiers::CTRL) => {
                term_window.cancel_modal();
            }
            (KeyCode::UpArrow, KeyModifiers::NONE)
            | (KeyCode::Char('k'), KeyModifiers::NONE)
            | (KeyCode::Char('p'), KeyModifiers::CTRL) => {
                self.move_up();
            }
            (KeyCode::DownArrow, KeyModifiers::NONE)
            | (KeyCode::Char('j'), KeyModifiers::NONE)
            | (KeyCode::Char('n'), KeyModifiers::CTRL) => {
                self.move_down();
            }
            (KeyCode::Enter, KeyModifiers::NONE) => {
                self.activate_selected(term_window);
                return Ok(true);
            }
            _ => return Ok(false),
        }
        term_window.invalidate_modal();
        Ok(true)
    }

    fn computed_element(
        &self,
        term_window: &mut TermWindow,
    ) -> anyhow::Result<Ref<'_, [ComputedElement]>> {
        if self.element.borrow().is_none() {
            let (element, row_height, menu_x, menu_y, menu_width, menu_height) = Self::compute(
                term_window,
                &self.items,
                *self.selected_row.borrow(),
                self.initial_mouse_x,
                self.initial_mouse_y,
            )?;
            self.element.borrow_mut().replace(element);
            *self.row_height.borrow_mut() = row_height;
            *self.menu_x.borrow_mut() = menu_x;
            *self.menu_y.borrow_mut() = menu_y;
            *self.menu_width.borrow_mut() = menu_width;
            *self.menu_height.borrow_mut() = menu_height;
        }
        Ok(Ref::map(self.element.borrow(), |v| {
            v.as_ref().unwrap().as_slice()
        }))
    }

    fn reconfigure(&self, _term_window: &mut TermWindow) {
        self.element.borrow_mut().take();
    }
}
