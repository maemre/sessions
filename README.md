# SQLx + Axum Lab

In this lab, you are going to extend the bookmark web app to add deletion and
update functionality.

To start, create the example database by running `./mk-db.sh`.

## Deleting a bookmark

1. Modify the detail page to add a link to a delete endpoint.
2. Create the delete endpoint, it should redirect to the `/bookmarks` page
   after success.
   - During deletion, you need to first delete the links (entries in the
     `bookmark_tag` table), and then the bookmark.
   - Use transactions to make sure this is atomic.
   - Use `rows_affected` to see whether a bookmark is deleted.

## Modifying a bookmark

1. Modify the detail page to add an update link that should go to the page in
   the next step.
2. Add a new template for a "modify bookmark" page, you can base this off of
   the "create bookmark" page.
3. Create endpoints for (a) this template, and (b) to handle the submission of
   the "modify bookmark" form.
   - When the form is submitted, the bookmark should be modified, and the
     client should be redirected to the bookmark's detail page.
