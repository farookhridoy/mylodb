/** Every keyboard shortcut that really works, grouped. The shortcuts window (Help → Keyboard Shortcuts, ⌘/) and the manual read this one list. */
export const SHORTCUT_GROUPS: { title: string; keys: [string, string][] }[] = [
  { title: "Everywhere", keys: [
    ["⌘K", "Command palette: every action, table and theme"],
    ["⌘,", "Preferences"],
    ["⌘B", "Show or hide the sidebar"],
    ["⇧⌘B", "Search objects in the sidebar"],
    ["F5", "Refresh the object tree"],
    ["⌘/", "This list of shortcuts"],
    ["Esc", "Close a window, or cancel an edit"],
  ] },
  { title: "Tabs and the SQL editor", keys: [
    ["⌘T", "New SQL script"],
    ["⌘W", "Close the tab"],
    ["⌘↵", "Run the selection, or the whole script"],
    ["⇧⌘F", "Format SQL"],
  ] },
  { title: "Object tree", keys: [
    ["F2", "Rename the selected table"],
    ["F4", "Open the selected table's properties"],
  ] },
  { title: "Data grid", keys: [
    ["Enter or F2", "Edit the cell (long text and JSON open in a window)"],
    ["Tab", "Save the cell and move right"],
    ["⌘↵", "Submit the pending changes"],
    ["⌘C", "Copy the cell"],
    ["Delete", "Delete the selected rows"],
    ["Arrows, PageUp, PageDown", "Move between cells"],
    ["⌘Home, ⌘End", "First and last row of the page"],
    ["⌘ click", "Follow a foreign key to the row it points to"],
  ] },
];
