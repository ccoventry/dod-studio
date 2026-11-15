import { describe, it, expect, beforeEach } from 'vitest';
import { setFieldError, clearFieldError, markFieldInvalid } from './field_error.js';

/** Just enough DOM for field_error: siblings, classes, attributes, listeners. */
function fakeElement(id = '') {
  const classes = new Set();
  const listeners = {};
  return {
    id,
    dataset: {},
    hidden: false,
    textContent: '',
    parent: null,
    attrs: {},
    classList: {
      add: (c) => classes.add(c),
      remove: (c) => classes.delete(c),
      toggle: (c, on) => (on ? classes.add(c) : classes.delete(c)),
      contains: (c) => classes.has(c),
    },
    set className(v) { classes.clear(); v.split(' ').forEach((c) => classes.add(c)); },
    setAttribute(k, v) { this.attrs[k] = v; },
    removeAttribute(k) { delete this.attrs[k]; },
    addEventListener(type, fn) { (listeners[type] ||= []).push(fn); },
    fire(type) { (listeners[type] || []).forEach((fn) => fn()); },
    closest() { return this.row || null; },
    get nextElementSibling() {
      const sibs = this.parent?.children || [];
      return sibs[sibs.indexOf(this) + 1] || null;
    },
    insertAdjacentElement(_where, el) {
      const sibs = this.parent.children;
      sibs.splice(sibs.indexOf(this) + 1, 0, el);
      el.parent = this.parent;
    },
  };
}

let parent;
function place(el) {
  el.parent = parent;
  parent.children.push(el);
  return el;
}

beforeEach(() => {
  parent = { children: [] };
  globalThis.document = { createElement: () => fakeElement() };
});

describe('setFieldError', () => {
  it('marks the field and shows the reason in a line right after it', () => {
    const input = place(fakeElement('a'));
    setFieldError(input, 'Fill this in.');
    expect(input.classList.contains('field-invalid')).toBe(true);
    expect(input.attrs['aria-invalid']).toBe('true');
    const line = input.nextElementSibling;
    expect(line.textContent).toBe('Fill this in.');
    expect(line.hidden).toBe(false);
  });

  it('puts the line under the whole row when the input sits in one', () => {
    const row = place(fakeElement('row'));
    const input = fakeElement('b');
    input.row = row;
    setFieldError(input, 'Nope.');
    expect(row.nextElementSibling.textContent).toBe('Nope.');
  });

  it('reuses its line on a second call', () => {
    const input = place(fakeElement('a'));
    setFieldError(input, 'one');
    setFieldError(input, 'two');
    expect(parent.children).toHaveLength(2);
    expect(input.nextElementSibling.textContent).toBe('two');
  });

  it('clears when the field is edited', () => {
    const input = place(fakeElement('a'));
    setFieldError(input, 'bad');
    input.fire('input');
    expect(input.classList.contains('field-invalid')).toBe(false);
    expect(input.nextElementSibling.hidden).toBe(true);
    expect(input.nextElementSibling.textContent).toBe('');
  });

  it('an empty message clears, and autoClear:false leaves clearing to the caller', () => {
    const input = place(fakeElement('a'));
    setFieldError(input, 'bad', { autoClear: false });
    input.fire('input');
    expect(input.classList.contains('field-invalid')).toBe(true);
    setFieldError(input, '', { autoClear: false });
    expect(input.classList.contains('field-invalid')).toBe(false);
  });
});

describe('clearFieldError / markFieldInvalid', () => {
  it('does nothing for a missing input or a field never marked', () => {
    expect(() => clearFieldError(null)).not.toThrow();
    const input = place(fakeElement('a'));
    expect(() => clearFieldError(input)).not.toThrow();
  });

  it('markFieldInvalid only toggles the red state', () => {
    const input = place(fakeElement('a'));
    markFieldInvalid(input, true);
    expect(input.attrs['aria-invalid']).toBe('true');
    markFieldInvalid(input, false);
    expect(input.attrs['aria-invalid']).toBeUndefined();
    expect(parent.children).toHaveLength(1);
  });
});
