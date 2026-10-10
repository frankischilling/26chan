'use strict';

var $ = {};

$.id = function(id) {
  return document.getElementById(id);
};

$.setToken = function() {
  document.cookie = '_ptkn=' + $.getToken()
    + '; path=/polls; domain=www.4chan.org; secure';
};

$.getToken = function() {
  return document.body.getAttribute('data-tkn');
};

$.on = function(n, e, h) {
  n.addEventListener(e, h, false);
};

$.off = function(n, e, h) {
  n.removeEventListener(e, h, false);
};

var APP = {
  init: function() {
    $.on(document, 'DOMContentLoaded', APP.run);
  },
  
  run: function() {
    var el;
    
    $.off(document, 'DOMContentLoaded', APP.run);
    
    if (el = $.id('poll-form')) {
      $.on(el, 'submit', $.setToken);
    }
  }
};

APP.init();
