use super::*;

pub(super) struct Drag {
    pub(super) start_address: u64,
    pub(super) end_address: u64,
}

impl Drag {
    pub(super) fn contains(&self, address: u64) -> bool {
        let range = self.memory_range();
        range.contains(&address)
    }

    pub(super) fn memory_range(&self) -> RangeInclusive<u64> {
        if self.start_address < self.end_address {
            self.start_address..=self.end_address
        } else {
            self.end_address..=self.start_address
        }
    }
}
#[derive(Clone, Debug)]
pub(super) enum SelectedMemoryRange {
    DragUnderway(Drag),
    DragComplete(Drag),
}

impl SelectedMemoryRange {
    pub(super) fn contains(&self, address: u64) -> bool {
        match self {
            SelectedMemoryRange::DragUnderway(drag) => drag.contains(address),
            SelectedMemoryRange::DragComplete(drag) => drag.contains(address),
        }
    }
    pub(super) fn is_dragging(&self) -> bool {
        matches!(self, SelectedMemoryRange::DragUnderway(_))
    }
    pub(super) fn drag(&self) -> &Drag {
        match self {
            SelectedMemoryRange::DragUnderway(drag) => drag,
            SelectedMemoryRange::DragComplete(drag) => drag,
        }
    }
}

#[derive(Clone)]
pub(super) struct ViewStateHandle(pub(super) Rc<RefCell<ViewState>>);

impl ViewStateHandle {
    pub(super) fn new(base_row: u64, line_width: ViewWidth) -> Self {
        Self(Rc::new(RefCell::new(ViewState::new(base_row, line_width))))
    }
}

#[derive(Clone)]
pub(super) struct ViewState {
    /// Uppermost row index
    pub(super) base_row: u64,
    /// How many cells per row do we have?
    pub(super) line_width: ViewWidth,
    pub(super) scroll_handle: UniformListScrollHandle,
    pub(super) selection: Option<SelectedMemoryRange>,
}

impl ViewState {
    pub(super) fn new(base_row: u64, line_width: ViewWidth) -> Self {
        Self {
            scroll_handle: UniformListScrollHandle::new(),
            base_row,
            line_width,
            selection: None,
        }
    }
    pub(super) fn row_count(&self) -> u64 {
        // This was picked fully arbitrarily. There's no incentive for us to care about page sizes other than the fact that it seems to be a good
        // middle ground for data size.
        const PAGE_SIZE: u64 = 4096;
        PAGE_SIZE / self.line_width.width as u64
    }
    pub(super) fn schedule_scroll_down(&mut self) {
        self.base_row = self.base_row.saturating_add(1)
    }
    pub(super) fn schedule_scroll_up(&mut self) {
        self.base_row = self.base_row.saturating_sub(1);
    }

    pub(super) fn set_offset(&mut self, point: Point<Pixels>) {
        if point.y >= -Pixels::ZERO {
            self.schedule_scroll_up();
        } else if point.y <= -self.scroll_handle.max_offset().y {
            self.schedule_scroll_down();
        }
        self.scroll_handle.set_offset(point);
    }
}

impl ScrollableHandle for ViewStateHandle {
    fn max_offset(&self) -> gpui::Point<Pixels> {
        self.0.borrow().scroll_handle.max_offset()
    }

    fn set_offset(&self, point: Point<Pixels>) {
        self.0.borrow_mut().set_offset(point);
    }

    fn offset(&self) -> Point<Pixels> {
        self.0.borrow().scroll_handle.offset()
    }

    fn viewport(&self) -> gpui::Bounds<Pixels> {
        self.0.borrow().scroll_handle.viewport()
    }
}

pub(super) static HEX_BYTES_MEMOIMAV: LazyLock<[SharedString; 256]> =
    LazyLock::new(|| std::array::from_fn(|byte| SharedString::from(format!("{byte:02X}"))));
pub(super) static UNKNOWN_BYTE: SharedString = SharedString::new_static("??");
